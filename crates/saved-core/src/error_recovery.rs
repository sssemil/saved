//! Error recovery and robust error handling for SAVED
//!
//! Provides comprehensive error recovery mechanisms, retry logic, and
//! graceful degradation for network operations, storage failures, and
//! synchronization issues.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Jitter factor to randomize delays
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }
}

/// Retry configuration for different operation types
pub struct RetryConfigs {
    /// Network operations (connections, requests)
    pub network: RetryConfig,
    /// Storage operations (read/write)
    pub storage: RetryConfig,
    /// Synchronization operations
    pub sync: RetryConfig,
    /// Chunk operations
    pub chunk: RetryConfig,
}

impl Default for RetryConfigs {
    fn default() -> Self {
        Self {
            network: RetryConfig {
                max_attempts: 5,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(60),
                backoff_multiplier: 2.0,
                jitter_factor: 0.2,
            },
            storage: RetryConfig {
                max_attempts: 3,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(10),
                backoff_multiplier: 1.5,
                jitter_factor: 0.1,
            },
            sync: RetryConfig {
                max_attempts: 10,
                initial_delay: Duration::from_millis(1000),
                max_delay: Duration::from_secs(120),
                backoff_multiplier: 2.0,
                jitter_factor: 0.3,
            },
            chunk: RetryConfig {
                max_attempts: 5,
                initial_delay: Duration::from_millis(200),
                max_delay: Duration::from_secs(30),
                backoff_multiplier: 1.8,
                jitter_factor: 0.15,
            },
        }
    }
}

/// Types of operations that can be retried
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
    Network,
    Storage,
    Sync,
    Chunk,
}

/// Circuit breaker state
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: u32,
    /// Time to wait before attempting to close the circuit
    pub timeout: Duration,
    /// Number of successful calls needed to close the circuit
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            timeout: Duration::from_secs(60),
            success_threshold: 3,
        }
    }
}

/// Circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    state: CircuitBreakerState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            config,
        }
    }

    pub fn can_execute(&self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    Instant::now().duration_since(last_failure) >= self.config.timeout
                } else {
                    true
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.failure_count = 0;

        if self.state == CircuitBreakerState::HalfOpen
            && self.success_count >= self.config.success_threshold
        {
            self.state = CircuitBreakerState::Closed;
            self.success_count = 0;
        }
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.success_count = 0;
        self.last_failure_time = Some(Instant::now());

        if self.failure_count >= self.config.failure_threshold {
            self.state = CircuitBreakerState::Open;
        }
    }

    pub fn get_state(&self) -> CircuitBreakerState {
        self.state.clone()
    }

    pub fn get_status(&self) -> CircuitBreakerStatus {
        CircuitBreakerStatus {
            state: self.state.clone(),
            failure_count: self.failure_count,
            success_count: self.success_count,
            last_failure_time: self.last_failure_time,
        }
    }
}

/// Statistics for retry operations
#[derive(Debug, Default, Clone)]
pub struct RetryStats {
    pub total_attempts: u64,
    pub successful_attempts: u64,
    pub failed_attempts: u64,
    pub circuit_breaker_trips: u64,
    pub average_retry_time: Duration,
}

/// Circuit breaker status for external queries
#[derive(Debug, Clone)]
pub struct CircuitBreakerStatus {
    pub state: CircuitBreakerState,
    pub failure_count: u32,
    pub success_count: u32,
    pub last_failure_time: Option<Instant>,
}

/// Error recovery manager
pub struct ErrorRecoveryManager {
    retry_configs: HashMap<OperationType, RetryConfig>,
    circuit_breakers: HashMap<String, CircuitBreaker>,
    retry_stats: RetryStats,
}

impl Default for ErrorRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorRecoveryManager {
    pub fn new() -> Self {
        let mut retry_configs = HashMap::new();
        let default_configs = RetryConfigs::default();

        retry_configs.insert(OperationType::Network, default_configs.network);
        retry_configs.insert(OperationType::Storage, default_configs.storage);
        retry_configs.insert(OperationType::Sync, default_configs.sync);
        retry_configs.insert(OperationType::Chunk, default_configs.chunk);

        Self {
            retry_configs,
            circuit_breakers: HashMap::new(),
            retry_stats: RetryStats::default(),
        }
    }

    /// Execute an operation with retry logic
    pub async fn execute_with_retry<F, T>(
        &mut self,
        operation_type: OperationType,
        operation_name: &str,
        operation: F,
    ) -> Result<T>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>
            + Send
            + Sync,
    {
        let config = self
            .retry_configs
            .get(&operation_type)
            .ok_or_else(|| Error::Network("Unknown operation type".to_string()))?
            .clone();

        // Check circuit breaker
        if !self.can_execute_operation(operation_name) {
            self.retry_stats.circuit_breaker_trips += 1;
            return Err(Error::Network("Circuit breaker is open".to_string()));
        }

        let mut attempt = 0;
        let mut delay = config.initial_delay;
        let start_time = Instant::now();

        loop {
            attempt += 1;
            self.retry_stats.total_attempts += 1;

            let operation_result = operation().await;

            match operation_result {
                Ok(result) => {
                    self.record_operation_success(operation_name);
                    self.retry_stats.successful_attempts += 1;

                    let total_time = start_time.elapsed();
                    self.update_average_retry_time(total_time);

                    return Ok(result);
                }
                Err(e) => {
                    self.record_operation_failure(operation_name);
                    self.retry_stats.failed_attempts += 1;

                    if attempt >= config.max_attempts {
                        return Err(e);
                    }

                    // Calculate delay with jitter
                    let jitter = fastrand::f64() * config.jitter_factor * delay.as_secs_f64();
                    let actual_delay = delay + Duration::from_secs_f64(jitter);

                    sleep(actual_delay).await;

                    // Exponential backoff
                    delay = Duration::from_millis(
                        (delay.as_millis() as f64 * config.backoff_multiplier) as u64,
                    )
                    .min(config.max_delay);
                }
            }
        }
    }

    /// Get retry statistics
    pub fn get_retry_stats(&self) -> &RetryStats {
        &self.retry_stats
    }

    /// Reset circuit breaker for a specific operation
    pub fn reset_circuit_breaker(&mut self, operation_name: &str) {
        if let Some(circuit_breaker) = self.circuit_breakers.get_mut(operation_name) {
            circuit_breaker.state = CircuitBreakerState::Closed;
            circuit_breaker.failure_count = 0;
            circuit_breaker.success_count = 0;
            circuit_breaker.last_failure_time = None;
        }
    }

    /// Get circuit breaker status for an operation
    pub fn get_circuit_breaker_status(&self, operation_name: &str) -> Option<CircuitBreakerStatus> {
        self.circuit_breakers
            .get(operation_name)
            .map(|cb| cb.get_status())
    }

    fn can_execute_operation(&self, operation_name: &str) -> bool {
        self.circuit_breakers
            .get(operation_name)
            .map(|cb| cb.can_execute())
            .unwrap_or(true)
    }

    fn record_operation_success(&mut self, operation_name: &str) {
        if let Some(circuit_breaker) = self.circuit_breakers.get_mut(operation_name) {
            circuit_breaker.record_success();
        }
    }

    fn record_operation_failure(&mut self, operation_name: &str) {
        if let Some(circuit_breaker) = self.circuit_breakers.get_mut(operation_name) {
            circuit_breaker.record_failure();
        } else {
            let mut circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
            circuit_breaker.record_failure();
            self.circuit_breakers
                .insert(operation_name.to_string(), circuit_breaker);
        }
    }

    fn update_average_retry_time(&mut self, new_time: Duration) {
        let total_attempts = self.retry_stats.total_attempts as f64;
        let current_avg = self.retry_stats.average_retry_time.as_millis() as f64;
        let new_avg =
            (current_avg * (total_attempts - 1.0) + new_time.as_millis() as f64) / total_attempts;
        self.retry_stats.average_retry_time = Duration::from_millis(new_avg as u64);
    }
}
