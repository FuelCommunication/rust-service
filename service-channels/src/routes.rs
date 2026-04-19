use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    schemas::{
        ChannelRead, ChannelReadPage, CreateChannel, PaginatedResponse, PaginationParams, SearchParams, SubscriberPage,
        SubscriptionPage, UpdateChannel,
    },
    state::ServerState,
};

fn extract_user_id(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    let value = headers
        .get("X-User-Id")
        .ok_or_else(|| ApiError::BadRequest("Missing X-User-Id header".into()))?
        .to_str()
        .map_err(|_| ApiError::BadRequest("Invalid X-User-Id header".into()))?;
    value
        .parse::<Uuid>()
        .map_err(|_| ApiError::BadRequest("X-User-Id is not a valid UUID".into()))
}

fn channel_key(id: Uuid) -> String {
    format!("channel:{id}")
}

fn sub_key(user_id: Uuid, channel_id: Uuid) -> String {
    format!("sub:{user_id}:{channel_id}")
}

#[tracing::instrument(skip(state, headers))]
pub async fn create_channel(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(data): Json<CreateChannel>,
) -> ApiResult<(StatusCode, Json<ChannelRead>)> {
    let user_id = extract_user_id(&headers)?;

    if data.title.is_empty() || data.title.len() > 255 {
        return Err(ApiError::BadRequest("Title must be between 1 and 255 characters".into()));
    }
    if let Some(ref description) = data.description
        && (description.is_empty() || description.len() > 300)
    {
        return Err(ApiError::BadRequest(
            "Description must be between 1 and 300 characters".into(),
        ));
    }

    let channel = state.store.create_channel(user_id, data).await?;
    let read = ChannelRead {
        id: channel.id,
        title: channel.title,
        description: channel.description,
        avatar_url: channel.avatar_url,
    };

    let _ = state.cache.set_ex(&channel_key(read.id), &read, state.cache_ttl).await;
    let _ = state.cache.set_ex(&sub_key(user_id, read.id), &true, state.cache_ttl).await;

    state
        .search
        .index_channel(read.id, &read.title, read.description.as_deref(), read.avatar_url.as_deref())
        .await;

    Ok((StatusCode::CREATED, Json(read)))
}

#[tracing::instrument(skip(state))]
pub async fn get_channels(
    State(state): State<ServerState>,
    Query(pagination): Query<PaginationParams>,
) -> ApiResult<Json<ChannelReadPage>> {
    let (rows, total) = state.store.list_channels(pagination.limit(), pagination.offset()).await?;

    Ok(Json(PaginatedResponse {
        items: rows
            .into_iter()
            .map(|c| ChannelRead {
                id: c.id,
                title: c.title,
                description: c.description,
                avatar_url: c.avatar_url,
            })
            .collect(),
        total,
        limit: pagination.limit(),
        offset: pagination.offset(),
    }))
}

#[tracing::instrument(skip(state))]
pub async fn get_channel(State(state): State<ServerState>, Path(channel_id): Path<Uuid>) -> ApiResult<Json<ChannelRead>> {
    let key = channel_key(channel_id);

    if let Ok(Some(cached)) = state.cache.get::<ChannelRead>(&key).await {
        return Ok(Json(cached));
    }

    let channel = state.store.get_channel(channel_id).await?;
    let read = ChannelRead {
        id: channel.id,
        title: channel.title,
        description: channel.description,
        avatar_url: channel.avatar_url,
    };

    let _ = state.cache.set_ex(&key, &read, state.cache_ttl).await;
    Ok(Json(read))
}

#[tracing::instrument(skip(state))]
pub async fn get_user_subscriptions(
    State(state): State<ServerState>,
    Path(user_id): Path<Uuid>,
    Query(pagination): Query<PaginationParams>,
) -> ApiResult<Json<SubscriptionPage>> {
    let (items, total) = state
        .store
        .get_user_subscriptions(user_id, pagination.limit(), pagination.offset())
        .await?;

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit: pagination.limit(),
        offset: pagination.offset(),
    }))
}

#[tracing::instrument(skip(state))]
pub async fn get_channel_subscribers(
    State(state): State<ServerState>,
    Path(channel_id): Path<Uuid>,
    Query(pagination): Query<PaginationParams>,
) -> ApiResult<Json<SubscriberPage>> {
    let (items, total) = state
        .store
        .get_channel_subscribers(channel_id, pagination.limit(), pagination.offset())
        .await?;

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit: pagination.limit(),
        offset: pagination.offset(),
    }))
}

#[tracing::instrument(skip(state, headers))]
pub async fn check_subscription(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(channel_id): Path<Uuid>,
) -> ApiResult<Json<bool>> {
    let user_id = extract_user_id(&headers)?;
    let key = sub_key(user_id, channel_id);

    if let Ok(Some(is_sub)) = state.cache.get::<bool>(&key).await {
        return Ok(Json(is_sub));
    }

    let is_sub = state.store.is_subscriber(user_id, channel_id).await?;
    let _ = state.cache.set_ex(&key, &is_sub, state.cache_ttl).await;

    Ok(Json(is_sub))
}

#[tracing::instrument(skip(state, headers))]
pub async fn update_channel(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(channel_id): Path<Uuid>,
    Json(data): Json<UpdateChannel>,
) -> ApiResult<Json<ChannelRead>> {
    let user_id = extract_user_id(&headers)?;

    if let Some(ref title) = data.title
        && (title.is_empty() || title.len() > 255)
    {
        return Err(ApiError::BadRequest("Title must be between 1 and 255 characters".into()));
    }
    if let Some(ref description) = data.description
        && (description.is_empty() || description.len() > 300)
    {
        return Err(ApiError::BadRequest(
            "Description must be between 1 and 300 characters".into(),
        ));
    }

    if !state.store.is_owner(user_id, channel_id).await? {
        return Err(ApiError::Forbidden("Only the channel owner can update this channel".into()));
    }

    let channel = state.store.update_channel(channel_id, data).await?;
    let read = ChannelRead {
        id: channel.id,
        title: channel.title,
        description: channel.description,
        avatar_url: channel.avatar_url,
    };

    let _ = state.cache.set_ex(&channel_key(channel_id), &read, state.cache_ttl).await;

    state
        .search
        .update_channel(read.id, &read.title, read.description.as_deref(), read.avatar_url.as_deref())
        .await;

    Ok(Json(read))
}

#[tracing::instrument(skip(state, headers))]
pub async fn delete_channel(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(channel_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user_id = extract_user_id(&headers)?;

    if !state.store.is_owner(user_id, channel_id).await? {
        return Err(ApiError::Forbidden("Only the channel owner can delete this channel".into()));
    }

    let subscriber_ids = state.store.get_all_subscriber_ids(channel_id).await?;

    state.store.delete_channel(channel_id).await?;

    let _ = state.cache.del(&channel_key(channel_id)).await;
    for sub_user_id in subscriber_ids {
        let _ = state.cache.del(&sub_key(sub_user_id, channel_id)).await;
    }

    state.search.delete_channel(channel_id).await;

    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state))]
pub async fn search_channels(
    State(state): State<ServerState>,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<ChannelReadPage>> {
    let (results, total) = state.search.search(&params.q, params.limit(), params.offset()).await?;

    let items = results
        .into_iter()
        .filter_map(|doc| {
            let id = doc.id.parse::<Uuid>().ok()?;
            Some(ChannelRead {
                id,
                title: doc.title,
                description: doc.description,
                avatar_url: doc.avatar_url,
            })
        })
        .collect();

    Ok(Json(PaginatedResponse {
        items,
        total,
        limit: params.limit(),
        offset: params.offset(),
    }))
}

#[tracing::instrument(skip(state, headers))]
pub async fn subscribe(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(channel_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user_id = extract_user_id(&headers)?;
    state.store.subscribe(user_id, channel_id).await?;

    let _ = state
        .cache
        .set_ex(&sub_key(user_id, channel_id), &true, state.cache_ttl)
        .await;

    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state, headers))]
pub async fn unsubscribe(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(channel_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user_id = extract_user_id(&headers)?;
    state.store.unsubscribe(user_id, channel_id).await?;
    let _ = state.cache.del(&sub_key(user_id, channel_id)).await;

    Ok(StatusCode::NO_CONTENT)
}

#[tracing::instrument(skip(state, headers))]
pub async fn transfer_ownership(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path((channel_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let user_id = extract_user_id(&headers)?;
    state.store.transfer_ownership(channel_id, user_id, target_user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
