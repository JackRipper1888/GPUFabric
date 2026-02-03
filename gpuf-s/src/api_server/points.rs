use crate::api_server::ApiServer;
use crate::util::msg::ApiResponse;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::error;
use validator::Validate;
use sqlx::Row;

// Request parameters for points query
#[derive(Debug, Deserialize, Validate)]
pub struct PointsQueryRequest {
    pub user_id: String,
    pub client_id: Option<String>,
    pub device_id: Option<i32>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    #[validate(range(min = 1, max = 100))]
    pub page: Option<i32>,
    #[validate(range(min = 1, max = 100))]
    pub page_size: Option<i32>,
}

// Response structure for individual device points
#[derive(Debug, Serialize)]
pub struct DevicePointsResponse {
    pub client_id: String,
    pub date: NaiveDate,
    pub total_heartbeats: i32,
    pub device_name: String,
    pub device_id: i32,
    pub points: f64,
}

// Response structure for points list with total summary
#[derive(Debug, Serialize)]
pub struct PointsListResponse {
    pub points: Vec<DevicePointsResponse>,
    pub total_points: f64,
    pub total_count: i64,
    pub page: i32,
    pub page_size: i32,
}

// Query device points for a user with optional filters
pub async fn get_user_points(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<PointsQueryRequest>,
) -> Result<Json<ApiResponse<PointsListResponse>>, StatusCode> {
    // Validate input
    if let Err(validation_errors) = params.validate() {
        error!("Validation errors: {:?}", validation_errors);
        return Err(StatusCode::BAD_REQUEST);
    }

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(20);
    let offset = (page - 1) * page_size;

    let client_id_bytes: Option<Vec<u8>> = if let Some(ref client_id) = params.client_id {
        let client_id = client_id.trim().trim_matches(|c| c == '\'' || c == '"');
        let bytes = hex::decode(client_id).map_err(|_| StatusCode::BAD_REQUEST)?;
        if bytes.len() != 16 {
            return Err(StatusCode::BAD_REQUEST);
        }
        Some(bytes)
    } else {
        None
    };

    // Build the base query with dynamic WHERE conditions
    let mut query_conditions = vec!["ga.user_id = $1".to_string()];
    let mut param_index = 2;

    // Add client_id filter if provided (hex string)
    if client_id_bytes.is_some() {
        query_conditions.push(format!("dpd.client_id = ${}", param_index));
        param_index += 1;
    }

    // Add device_id filter if provided
    if params.device_id.is_some() {
        query_conditions.push(format!("dpd.device_id = ${}", param_index));
        param_index += 1;
    }

    // Add date range filters if provided
    if params.start_date.is_some() {
        query_conditions.push(format!("dpd.date >= ${}", param_index));
        param_index += 1;
    }

    if params.end_date.is_some() {
        query_conditions.push(format!("dpd.date <= ${}", param_index));
        param_index += 1;
    }

    let where_clause = query_conditions.join(" AND ");

    // Main query to get paginated results with total summary
    let query = format!(r#"
        WITH filtered_points AS (
            SELECT 
                encode(dpd.client_id::bytea, 'hex') as client_id,
                dpd.date,
                dpd.total_heartbeats,
                COALESCE(dpd.device_name, 'Unknown Device') as device_name,
                COALESCE(dpd.device_id, 0) as device_id,
                (dpd.points)::DOUBLE PRECISION as points,
                (SUM(dpd.points) OVER ())::DOUBLE PRECISION as total_points,
                COUNT(*) OVER () as total_count,
                ROW_NUMBER() OVER (ORDER BY dpd.date DESC, dpd.client_id) as row_num
            FROM device_points_daily dpd
            INNER JOIN gpu_assets ga ON dpd.client_id = ga.client_id
            WHERE {}
        )
        SELECT 
            client_id,
            date,
            total_heartbeats,
            device_name,
            device_id,
            points,
            total_points,
            total_count
        FROM filtered_points
        WHERE row_num > ${} AND row_num <= ${}
        ORDER BY date DESC, client_id
    "#, where_clause, param_index, param_index + 1);

    // Execute query with parameters
    let mut query_builder = sqlx::query(&query);
    
    // Bind user_id (first parameter)
    query_builder = query_builder.bind(&params.user_id);
    
    // Bind optional parameters
    if let Some(client_id_bytes) = client_id_bytes {
        query_builder = query_builder.bind(client_id_bytes);
    }
    if let Some(device_id) = params.device_id {
        query_builder = query_builder.bind(device_id);
    }
    if let Some(ref start_date) = params.start_date {
        query_builder = query_builder.bind(start_date);
    }
    if let Some(ref end_date) = params.end_date {
        query_builder = query_builder.bind(end_date);
    }
    
    // Bind pagination parameters
    query_builder = query_builder.bind(offset);
    query_builder = query_builder.bind(offset + page_size);

    // Execute the query
    let rows = match query_builder
        .fetch_all(&app_state.db_pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            error!("Failed to query user points: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Process results
    if rows.is_empty() {
        return Ok(Json(ApiResponse::success(PointsListResponse {
            points: Vec::new(),
            total_points: 0.0,
            total_count: 0,
            page,
            page_size,
        })));
    }

    // Convert rows to response format
    let mut points = Vec::new();
    let mut total_points = 0.0;
    let mut total_count = 0i64;
    let mut summary_set = false;

    for row in rows {
        let client_id: String = row.get("client_id");
        let date: NaiveDate = row.get("date");
        let total_heartbeats: i32 = row.get("total_heartbeats");
        let device_name: String = row.get("device_name");
        let device_id: i32 = row.get("device_id");
        let points_value: f64 = row.get("points");
        
        // Get total_points and total_count from first row
        if !summary_set {
            total_points = row.get("total_points");
            total_count = row.get("total_count");
            summary_set = true;
        }

        points.push(DevicePointsResponse {
            client_id,
            date,
            total_heartbeats,
            device_name,
            device_id,
            points: points_value,
        });
    }

    Ok(Json(ApiResponse::success(PointsListResponse {
        points,
        total_points,
        total_count,
        page,
        page_size,
    })))
}
