// Domain types for the Stock Analysis Agent
// This file shows the complete type definitions needed

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Real-time stock quote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQuote {
    pub symbol: String,
    pub price: f64,
    pub change: f64,
    pub change_percent: f64,
    pub volume: u64,
    pub market_cap: Option<f64>,
    pub high_52_week: Option<f64>,
    pub low_52_week: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

/// Comprehensive stock analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockAnalysis {
    pub symbol: String,
    pub current_price: f64,
    pub recommendation: Recommendation,
    pub technical_indicators: TechnicalIndicators,
    pub fundamental_data: FundamentalData,
    pub risk_level: RiskLevel,
    pub analysis_time: DateTime<Utc>,
}

/// Buy/Sell recommendation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Recommendation {
    StrongBuy,
    Buy,
    Hold,
    Sell,
    StrongSell,
}

/// Technical analysis indicators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalIndicators {
    /// Simple Moving Average (20 periods)
    pub sma_20: Option<f64>,
    /// Simple Moving Average (50 periods)
    pub sma_50: Option<f64>,
    /// Relative Strength Index (14 periods)
    pub rsi: Option<f64>,
    /// Moving Average Convergence Divergence
    pub macd: Option<MacdIndicator>,
}

/// MACD indicator components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacdIndicator {
    pub macd: f64,
    pub signal: f64,
    pub histogram: f64,
}

/// Fundamental analysis data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundamentalData {
    /// Price-to-Earnings ratio
    pub pe_ratio: Option<f64>,
    /// Earnings Per Share
    pub eps: Option<f64>,
    /// Market capitalization
    pub market_cap: Option<f64>,
    /// Annual revenue
    pub revenue: Option<f64>,
    /// Profit margin percentage
    pub profit_margin: Option<f64>,
}

/// Risk assessment level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    VeryHigh,
    Unknown,
}

/// Query intent classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    /// Simple price quote
    Quote,
    /// Full technical/fundamental analysis
    Analyze,
    /// Historical price data
    Historical,
    /// Compare multiple stocks
    Compare,
    /// Set up price alerts
    Watch,
}

/// Candle/OHLCV data point (re-export from yfinance-rs)
pub use yfinance_rs::Candle;

/// Time period for historical data (re-export from yfinance-rs)
pub use yfinance_rs::Period;

// Example usage in your handler:
//
// let quote = StockQuote {
//     symbol: "AAPL".to_string(),
//     price: 178.42,
//     change: 2.15,
//     change_percent: 1.22,
//     volume: 52_341_892,
//     market_cap: Some(2_800_000_000_000.0),
//     high_52_week: Some(198.23),
//     low_52_week: Some(124.17),
//     timestamp: Utc::now(),
// };
//
// let analysis = StockAnalysis {
//     symbol: "AAPL".to_string(),
//     current_price: 178.42,
//     recommendation: Recommendation::Buy,
//     technical_indicators: TechnicalIndicators {
//         sma_20: Some(175.30),
//         sma_50: Some(172.15),
//         rsi: Some(58.32),
//         macd: Some(MacdIndicator {
//             macd: 1.245,
//             signal: 0.982,
//             histogram: 0.263,
//         }),
//     },
//     fundamental_data: FundamentalData {
//         pe_ratio: Some(29.45),
//         eps: Some(6.05),
//         market_cap: Some(2_800_000_000_000.0),
//         revenue: Some(383_285_000_000.0),
//         profit_margin: Some(0.258),
//     },
//     risk_level: RiskLevel::Medium,
//     analysis_time: Utc::now(),
// };
