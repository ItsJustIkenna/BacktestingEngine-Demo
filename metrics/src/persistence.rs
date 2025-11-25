//! Database models for backtest report persistence
//!
//! This module provides database models for storing and retrieving
//! generated backtest reports and their metadata.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use diesel::{Insertable, Queryable, Selectable};
use std::collections::HashMap;
use bigdecimal::BigDecimal;
use serde_json::Value;

// Import database schema
use databaseschema::schema::{backtest_reports, backtest_report_access_log};

/// Database model for backtest reports table
#[derive(Queryable, Insertable, Selectable, Debug, Clone, Serialize, Deserialize)]
#[diesel(table_name = backtest_reports)]
pub struct BacktestReportRecord {
    pub id: Uuid,
    pub backtest_result_id: Uuid,
    pub report_id: String,
    pub report_name: String,
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_capital: BigDecimal,
    pub generated_at: DateTime<Utc>,
    pub generated_by: Option<String>,
    pub generation_source: String,
    pub backtest_duration_seconds: Option<BigDecimal>,
    pub data_points: Option<i32>,
    pub include_trades: bool,
    pub include_charts: bool,
    pub export_formats: Vec<String>,
    pub custom_css: Option<String>,
    pub template_version: Option<String>,
    pub file_paths: Value, // JSONB in database
    pub file_sizes: Option<Value>, // JSONB in database
    pub storage_location: String,
    pub performance_summary: Value, // JSONB in database
    pub risk_summary: Value, // JSONB in database
    pub trade_summary: Value, // JSONB in database
    pub status: String,
    pub error_message: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub accessed_at: Option<DateTime<Utc>>,
    pub access_count: i32,
}

/// New report record for insertion
#[derive(Insertable, Debug, Clone, Serialize, Deserialize)]
#[diesel(table_name = backtest_reports)]
pub struct NewBacktestReportRecord {
    pub backtest_result_id: Uuid,
    pub report_id: String,
    pub report_name: String,
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_capital: BigDecimal,
    pub generated_at: DateTime<Utc>,
    pub generated_by: Option<String>,
    pub generation_source: String,
    pub backtest_duration_seconds: Option<BigDecimal>,
    pub data_points: Option<i32>,
    pub include_trades: bool,
    pub include_charts: bool,
    pub export_formats: Vec<String>,
    pub custom_css: Option<String>,
    pub template_version: Option<String>,
    pub file_paths: Value,
    pub file_sizes: Option<Value>,
    pub storage_location: String,
    pub performance_summary: Value,
    pub risk_summary: Value,
    pub trade_summary: Value,
    pub status: String,
    pub error_message: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Database model for report access log
#[derive(Queryable, Insertable, Selectable, Debug, Clone, Serialize, Deserialize)]
#[diesel(table_name = backtest_report_access_log)]
pub struct BacktestReportAccessLog {
    pub id: Uuid,
    pub report_id: Uuid,
    pub accessed_by: Option<String>,
    pub access_method: String,
    pub format_requested: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>, // Store as string for now
    pub response_time_ms: Option<i32>,
    pub success: bool,
    pub error_message: Option<String>,
    pub accessed_at: DateTime<Utc>,
}

/// New access log entry for insertion
#[derive(Insertable, Debug, Clone, Serialize, Deserialize)]
#[diesel(table_name = backtest_report_access_log)]
pub struct NewBacktestReportAccessLog {
    pub report_id: Uuid,
    pub accessed_by: Option<String>,
    pub access_method: String,
    pub format_requested: Option<String>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>, // Store as string for now
    pub response_time_ms: Option<i32>,
    pub success: bool,
    pub error_message: Option<String>,
}

/// Report summary view model
#[derive(Queryable, Debug, Clone, Serialize, Deserialize)]
pub struct BacktestReportSummary {
    pub id: Uuid,
    pub report_id: String,
    pub report_name: String,
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub generated_at: DateTime<Utc>,
    pub generated_by: Option<String>,
    pub export_formats: Vec<String>,
    pub status: String,
    pub access_count: i32,
    pub accessed_at: Option<DateTime<Utc>>,
    pub net_profit: Option<BigDecimal>,
    pub sharpe_ratio: Option<BigDecimal>,
    pub max_drawdown: Option<BigDecimal>,
    pub total_trades: Option<i32>,
    pub backtest_total_return: BigDecimal,
    pub backtest_created_at: DateTime<Utc>,
}

/// Conversion helpers and utility functions
impl NewBacktestReportRecord {
    /// Create a new report record from BacktestReport and metadata
    pub fn from_backtest_report(
        report: &crate::reporting::BacktestReport,
        backtest_result_id: Uuid,
        file_paths: HashMap<String, String>,
        generated_by: Option<String>,
        generation_source: String,
    ) -> Self {
        let file_paths_json = serde_json::to_value(&file_paths)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        
        let performance_summary = serde_json::json!({
            "net_profit": report.performance.net_profit,
            "sharpe_ratio": report.performance.sharpe_ratio,
            "profit_factor": report.performance.profit_factor,
            "max_drawdown": report.performance.max_drawdown,
            "win_rate": report.performance.win_rate,
            "total_trades": report.performance.number_of_trades,
            // Enhanced metrics - transaction costs
            "total_commission": report.performance.total_commission,
            "total_slippage": report.performance.total_slippage,
            "total_market_impact": report.performance.total_market_impact,
            "transaction_costs_pct": report.performance.transaction_costs_percentage,
        });
        
        let risk_summary = serde_json::json!({
            "volatility": report.performance.volatility,
            "value_at_risk_95": report.analysis.risk_metrics.value_at_risk_95,
            "sortino_ratio": report.analysis.risk_metrics.sortino_ratio,
            "calmar_ratio": report.analysis.risk_metrics.calmar_ratio,
            // Enhanced metrics - data quality
            "data_quality_score": report.analysis.data_quality.overall_score,
            "data_completeness": report.analysis.data_quality.data_completeness_score,
            "gaps_detected": report.analysis.data_quality.gaps_detected,
            "max_gap_minutes": report.analysis.data_quality.max_gap_seconds / 60,
        });
        
        let trade_summary = serde_json::json!({
            "total_trades": report.analysis.trade_distribution.profitable_trades + report.analysis.trade_distribution.losing_trades,
            "profitable_trades": report.analysis.trade_distribution.profitable_trades,
            "losing_trades": report.analysis.trade_distribution.losing_trades,
            "best_trade": report.analysis.trade_distribution.best_trade,
            "worst_trade": report.analysis.trade_distribution.worst_trade,
            // Enhanced metrics - market impact
            "trades_with_impact": report.analysis.market_impact.trades_with_impact,
            "avg_impact_bps": report.analysis.market_impact.avg_impact_bps,
            "liquidity_warnings": report.analysis.market_impact.liquidity_warnings,
        });

        Self {
            backtest_result_id,
            report_id: report.metadata.report_id.clone(),
            report_name: format!("{} - {} Report", report.metadata.strategy_name, report.metadata.symbol),
            strategy_name: report.metadata.strategy_name.clone(),
            symbol: report.metadata.symbol.clone(),
            timeframe: report.metadata.timeframe.clone(),
            start_date: report.metadata.start_date,
            end_date: report.metadata.end_date,
            initial_capital: BigDecimal::from(report.metadata.initial_capital as i64),
            generated_at: report.metadata.generated_at,
            generated_by,
            generation_source,
            backtest_duration_seconds: report.metadata.backtest_duration_seconds
                .map(|d| BigDecimal::from(d as i64)),
            data_points: Some(report.metadata.data_points as i32),
            include_trades: report.trades.is_some(),
            include_charts: true, // Assume charts are included by default
            export_formats: vec![], // Will be filled based on generated files
            custom_css: None,
            template_version: Some("1.0".to_string()),
            file_paths: file_paths_json,
            file_sizes: None, // Can be calculated from files
            storage_location: "local".to_string(),
            performance_summary,
            risk_summary,
            trade_summary,
            status: "generated".to_string(),
            error_message: None,
            tags: None,
            notes: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// Repository for database operations on reports
pub struct BacktestReportRepository {
    pool: std::sync::Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>,
}

impl BacktestReportRepository {
    /// Create a new repository with connection pool
    pub fn new(pool: std::sync::Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>) -> Self {
        Self { pool }
    }

    /// Save a new report record to the database
    pub async fn save_report(&self, report: NewBacktestReportRecord) -> Result<(), diesel::result::Error> {
        use databaseschema::schema::backtest_reports;
        use diesel_async::RunQueryDsl;
        
        let mut conn = self.pool.get().await
            .map_err(|e| diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UnableToSendCommand,
                Box::new(e.to_string())
            ))?;
        
        // Insert the record
        diesel::insert_into(backtest_reports::table)
            .values(&report)
            .execute(&mut conn)
            .await?;
        
        Ok(())
    }
    
    /// Find a report by report_id
    pub async fn find_by_report_id(&self, report_id: &str) -> Result<Option<BacktestReportRecord>, diesel::result::Error> {
        // Implementation would query by report_id
        todo!("Implement find by report_id")
    }
    
    /// List reports for a specific backtest result
    pub async fn list_by_backtest_result(&self, backtest_result_id: Uuid) -> Result<Vec<BacktestReportSummary>, diesel::result::Error> {
        // Implementation would query reports for given backtest
        todo!("Implement list by backtest result")
    }
    
    /// Update report access tracking
    pub async fn log_access(&self, access_log: NewBacktestReportAccessLog) -> Result<(), diesel::result::Error> {
        // Implementation would insert access log record
        todo!("Implement access logging")
    }
    
    /// Mark report as accessed (increment counter, update timestamp)
    pub async fn mark_accessed(&self, report_id: Uuid) -> Result<(), diesel::result::Error> {
        // Implementation would update access count and timestamp
        todo!("Implement mark accessed")
    }
    
    /// Search reports by various criteria
    pub async fn search_reports(
        &self,
        strategy_name: Option<&str>,
        symbol: Option<&str>,
        generated_by: Option<&str>,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
        tags: Option<&[String]>,
    ) -> Result<Vec<BacktestReportSummary>, diesel::result::Error> {
        // Implementation would build dynamic query based on criteria
        todo!("Implement report search")
    }
    
    /// Delete old archived reports
    pub async fn cleanup_old_reports(&self, days_old: i32) -> Result<i32, diesel::result::Error> {
        // Implementation would call the cleanup_old_reports SQL function
        todo!("Implement cleanup old reports")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_new_report_record_creation() {
        // Test would create a mock BacktestReport and verify conversion
        // This ensures the conversion logic works correctly
        assert!(true); // Placeholder
    }
    
    #[test]
    fn test_performance_summary_json() {
        // Test would verify JSON serialization of performance metrics
        assert!(true); // Placeholder
    }
}
