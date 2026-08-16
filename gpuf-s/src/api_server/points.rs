use crate::api_server::ApiServer;
use crate::util::msg::ApiResponse;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use tracing::error;
use validator::Validate;

// Request parameters for points query
#[derive(Debug, Deserialize, Validate)]
pub struct PointsQueryRequest {
    #[validate(length(min = 1, max = 64))]
    pub user_id: String,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub device_id: Option<i32>,
    pub device_index: Option<i16>,
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
    pub client_name: String,
    pub date: NaiveDate,
    pub total_heartbeats: i32,
    pub device_name: String,
    pub device_id: i32,
    pub device_index: i16,
    pub contributed_hours: f64,
    pub tflops: Option<f64>,
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

// Request parameters for the points summary shown at the top of the points center.
#[derive(Debug, Deserialize, Validate)]
pub struct PointsSummaryQueryRequest {
    #[validate(length(min = 1, max = 64))]
    pub user_id: String,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub device_id: Option<i32>,
    pub device_index: Option<i16>,
}

#[derive(Debug, Serialize)]
pub struct PointsSummaryResponse {
    pub total_points: f64,
    pub today_points: f64,
    pub month_points: f64,
    pub as_of_date: NaiveDate,
}

type PointsApiError = (StatusCode, Json<ApiResponse<()>>);

fn bad_request(message: impl Into<String>) -> PointsApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::<()>::error(message.into())),
    )
}

fn validate_user_id(user_id: &str) -> Result<(), PointsApiError> {
    if user_id.trim().is_empty() {
        return Err(bad_request("user_id must not be empty"));
    }
    Ok(())
}

fn validate_date_range(
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Result<(), PointsApiError> {
    if matches!((start_date, end_date), (Some(start), Some(end)) if start > end) {
        return Err(bad_request("start_date must not be later than end_date"));
    }
    Ok(())
}

fn decode_client_id(client_id: Option<&str>) -> Result<Option<Vec<u8>>, PointsApiError> {
    let Some(client_id) = client_id else {
        return Ok(None);
    };
    let client_id = client_id.trim().trim_matches(|c| c == '\'' || c == '"');
    let bytes = hex::decode(client_id)
        .map_err(|_| bad_request("invalid client_id: expected 32-char hex string"))?;
    if bytes.len() != 16 {
        return Err(bad_request(
            "invalid client_id: expected 16 bytes (32 hex chars)",
        ));
    }
    Ok(Some(bytes))
}

// Query the user's cumulative, current-day, and current-month device points.
pub async fn get_user_points_summary(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<PointsSummaryQueryRequest>,
) -> Result<Json<ApiResponse<PointsSummaryResponse>>, PointsApiError> {
    if let Err(validation_errors) = params.validate() {
        return Err(bad_request(format!(
            "validation errors: {:?}",
            validation_errors
        )));
    }
    validate_user_id(&params.user_id)?;

    let client_id_bytes = decode_client_id(params.client_id.as_deref())?;
    let client_name_filter = params
        .client_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let mut query_conditions = vec![
        "ga.user_id = $1".to_string(),
        "dpd.points > 0".to_string(),
        "dpd.date <= CURRENT_DATE".to_string(),
    ];
    let mut param_index = 2;

    if client_id_bytes.is_some() {
        query_conditions.push(format!("dpd.client_id = ${param_index}"));
        param_index += 1;
    }
    if client_name_filter.is_some() {
        query_conditions.push(format!("COALESCE(ga.client_name, '') ILIKE ${param_index}"));
        param_index += 1;
    }
    if params.device_id.is_some() {
        query_conditions.push(format!("dpd.device_id = ${param_index}"));
        param_index += 1;
    }
    if params.device_index.is_some() {
        query_conditions.push(format!("dpd.device_index = ${param_index}"));
    }

    let query = format!(
        r#"
        SELECT
            CURRENT_DATE AS as_of_date,
            (COALESCE(FLOOR(SUM(dpd.points) * 100), 0) / 100.0)::DOUBLE PRECISION
                AS total_points,
            (COALESCE(FLOOR(SUM(dpd.points) FILTER (
                WHERE dpd.date = CURRENT_DATE
            ) * 100), 0) / 100.0)::DOUBLE PRECISION AS today_points,
            (COALESCE(FLOOR(SUM(dpd.points) FILTER (
                WHERE dpd.date >= DATE_TRUNC('month', CURRENT_DATE)::DATE
            ) * 100), 0) / 100.0)::DOUBLE PRECISION AS month_points
        FROM public.device_points_daily dpd
        INNER JOIN public.gpu_assets ga ON dpd.client_id = ga.client_id
        WHERE {}
        "#,
        query_conditions.join(" AND ")
    );

    let mut query_builder = sqlx::query(&query).bind(&params.user_id);
    if let Some(client_id_bytes) = client_id_bytes {
        query_builder = query_builder.bind(client_id_bytes);
    }
    if let Some(client_name_filter) = client_name_filter {
        query_builder = query_builder.bind(format!("%{client_name_filter}%"));
    }
    if let Some(device_id) = params.device_id {
        query_builder = query_builder.bind(device_id);
    }
    if let Some(device_index) = params.device_index {
        query_builder = query_builder.bind(device_index);
    }

    let row = query_builder
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| {
            error!("Failed to query user points summary: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(
                    "internal server error".to_string(),
                )),
            )
        })?;

    Ok(Json(ApiResponse::success(PointsSummaryResponse {
        total_points: row.get("total_points"),
        today_points: row.get("today_points"),
        month_points: row.get("month_points"),
        as_of_date: row.get("as_of_date"),
    })))
}

// Query device points for a user with optional filters
pub async fn get_user_points(
    State(app_state): State<Arc<ApiServer>>,
    Query(params): Query<PointsQueryRequest>,
) -> Result<Json<ApiResponse<PointsListResponse>>, (StatusCode, Json<ApiResponse<()>>)> {
    // Validate input
    if let Err(validation_errors) = params.validate() {
        error!("Validation errors: {:?}", validation_errors);
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(format!(
                "validation errors: {:?}",
                validation_errors
            ))),
        ));
    }

    validate_user_id(&params.user_id)?;
    validate_date_range(params.start_date, params.end_date)?;

    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(20);
    let offset = (page - 1) * page_size;

    let client_name_filter: Option<String> = params
        .client_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let client_id_bytes = decode_client_id(params.client_id.as_deref())?;

    // Build the base query with dynamic WHERE conditions
    let mut query_conditions = vec!["ga.user_id = $1".to_string(), "dpd.points > 0".to_string()];
    let mut param_index = 2;

    // Add client_id filter if provided (hex string)
    if client_id_bytes.is_some() {
        query_conditions.push(format!("dpd.client_id = ${}", param_index));
        param_index += 1;
    }

    // Add client_name fuzzy filter if provided
    if client_name_filter.is_some() {
        query_conditions.push(format!(
            "COALESCE(ga.client_name, '') ILIKE ${}",
            param_index
        ));
        param_index += 1;
    }

    // Add device_id filter if provided
    if params.device_id.is_some() {
        query_conditions.push(format!("dpd.device_id = ${}", param_index));
        param_index += 1;
    }

    // device_id identifies a GPU model; device_index identifies one physical
    // device within a client and can be combined with client_id for exact filtering.
    if params.device_index.is_some() {
        query_conditions.push(format!("dpd.device_index = ${}", param_index));
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
    let query = format!(
        r#"
        WITH base_points AS (
            SELECT
                encode(dpd.client_id::bytea, 'hex') as client_id,
                COALESCE(ga.client_name, '') as client_name,
                dpd.date,
                dpd.total_heartbeats,
                COALESCE(dpd.device_name, '-') as device_name,
                COALESCE(dpd.device_id, 0) as device_id,
                dpd.device_index,
                dpd.base_hours::DOUBLE PRECISION as contributed_hours,
                dpd.tflops,
                dpd.points as raw_points,
                SUM(dpd.points * 100) OVER (
                    ORDER BY dpd.date ASC, dpd.client_id ASC, dpd.device_index ASC
                    ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                ) as cumulative_scaled_points,
                SUM(dpd.points * 100) OVER () as total_scaled_points,
                COUNT(*) OVER () as total_count
            FROM public.device_points_daily dpd
            INNER JOIN public.gpu_assets ga ON dpd.client_id = ga.client_id
            WHERE {}
        ),
        filtered_points AS (
            SELECT
                client_id,
                client_name,
                date,
                total_heartbeats,
                device_name,
                device_id,
                device_index,
                contributed_hours,
                tflops,
                ((FLOOR(cumulative_scaled_points) - FLOOR(cumulative_scaled_points - raw_points * 100)) / 100.0)::DOUBLE PRECISION as points,
                (FLOOR(total_scaled_points) / 100.0)::DOUBLE PRECISION as total_points,
                total_count,
                ROW_NUMBER() OVER (ORDER BY date DESC, client_id, device_index) as row_num
            FROM base_points
        )
        SELECT 
            client_id,
            client_name,
            date,
            total_heartbeats,
            device_name,
            device_id,
            device_index,
            contributed_hours,
            tflops,
            points,
            total_points,
            total_count
        FROM filtered_points
        WHERE row_num > ${} AND row_num <= ${}
        ORDER BY date DESC, client_id
    "#,
        where_clause,
        param_index,
        param_index + 1
    );

    // Execute query with parameters
    let mut query_builder = sqlx::query(&query);

    // Bind user_id (first parameter)
    query_builder = query_builder.bind(&params.user_id);

    // Bind optional parameters
    if let Some(client_id_bytes) = client_id_bytes {
        query_builder = query_builder.bind(client_id_bytes);
    }
    if let Some(client_name_filter) = client_name_filter {
        query_builder = query_builder.bind(format!("%{}%", client_name_filter));
    }
    if let Some(device_id) = params.device_id {
        query_builder = query_builder.bind(device_id);
    }
    if let Some(device_index) = params.device_index {
        query_builder = query_builder.bind(device_index);
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
    let rows = match query_builder.fetch_all(&app_state.db_pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!("Failed to query user points: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(
                    "internal server error".to_string(),
                )),
            ));
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
        let client_name: String = row.get("client_name");
        let date: NaiveDate = row.get("date");
        let total_heartbeats: i32 = row.get("total_heartbeats");
        let device_name: String = row.get("device_name");
        let device_id: i32 = row.get("device_id");
        let device_index: i16 = row.get("device_index");
        let contributed_hours: f64 = row.get("contributed_hours");
        let tflops: Option<f64> = row.get("tflops");
        let points_value: f64 = row.get("points");

        // Get total_points and total_count from first row
        if !summary_set {
            total_points = row.get("total_points");
            total_count = row.get("total_count");
            summary_set = true;
        }

        points.push(DevicePointsResponse {
            client_id,
            client_name,
            date,
            total_heartbeats,
            device_name,
            device_id,
            device_index,
            contributed_hours,
            tflops,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_must_be_exactly_16_bytes() {
        assert!(decode_client_id(Some("00112233445566778899aabbccddeeff")).is_ok());
        assert!(decode_client_id(Some("00112233")).is_err());
        assert!(decode_client_id(Some("not-hex")).is_err());
    }

    #[test]
    fn reversed_date_range_is_rejected() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 8, 2).unwrap();
        assert!(validate_date_range(Some(start), Some(end)).is_err());
        assert!(validate_date_range(Some(end), Some(start)).is_ok());
    }
}
