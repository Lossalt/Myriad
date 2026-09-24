//! Process-owned periodic background jobs.
//!
//! One cancellation root and one join set for every loop the web process runs
//! outside a request. Each job is a serial tick: slow work delays its own next
//! tick (missed ticks are skipped) and never overlaps itself or holds another job.
//!
//! Panic policy: a panicking tick is caught, logged with the job name, and the
//! job resumes at its next tick. Every tick is a fresh future from the job's
//! factory, so a panic loses only that tick's work; stopping the loop instead
//! would silently end a scheduler for the rest of the process lifetime.
//!
//! Shutdown cancels admission first, then waits for in-flight ticks up to a
//! deadline and aborts what is left. A tick is never interrupted before the
//! deadline, so work with a durable lease finishes or leaves the lease to expire.
//!
//! The persona runtime keeps its own supervisor (`persona::drivers`): there a
//! stopped driver is a worker failure that restarts the process, which is a
//! different contract from these best-effort maintenance loops.

use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// When a periodic job runs.
#[derive(Clone, Copy, Debug)]
pub struct Every {
    period: Duration,
    first_delay: Duration,
    jitter: Duration,
}

impl Every {
    /// First tick runs immediately, then every `period`.
    pub const fn new(period: Duration) -> Self {
        Self {
            period,
            first_delay: Duration::ZERO,
            jitter: Duration::ZERO,
        }
    }

    /// Delay the first tick.
    pub const fn after(mut self, first_delay: Duration) -> Self {
        self.first_delay = first_delay;
        self
    }

    /// Wait a random `0..=jitter` before each tick, so replicas sharing a
    /// database lease do not all wake on the same instant.
    pub const fn jitter(mut self, jitter: Duration) -> Self {
        self.jitter = jitter;
        self
    }
}

/// Stops one job without touching the others. Dropping it does not stop the job.
#[derive(Debug)]
pub struct JobHandle(CancellationToken);

impl JobHandle {
    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

pub struct JobRunner {
    root: CancellationToken,
    tasks: Mutex<JoinSet<&'static str>>,
}

impl Default for JobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRunner {
    /// Needs no runtime; jobs are spawned when registered.
    pub fn new() -> Self {
        Self {
            root: CancellationToken::new(),
            tasks: Mutex::new(JoinSet::new()),
        }
    }

    /// Register a periodic job. After [`JobRunner::shutdown`] started, the job
    /// is refused and the returned handle is already cancelled.
    pub fn periodic<F, Fut>(&self, name: &'static str, every: Every, work: F) -> JobHandle
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cancel = self.root.child_token();
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        // Checked under the lock: shutdown cancels before it takes the set, so
        // nothing can slip in after the drain started.
        if self.root.is_cancelled() {
            tracing::warn!(
                job = name,
                "background job refused: runner is shutting down"
            );
            return JobHandle(cancel);
        }
        // Stopped jobs (JobHandle::cancel) leave results behind; reap them here
        // so start/stop cycles do not grow the set.
        while let Some(finished) = tasks.try_join_next() {
            log_join_result(finished);
        }
        let token = cancel.clone();
        tasks.spawn(async move {
            run_periodic(name, token, every, work).await;
            name
        });
        JobHandle(cancel)
    }

    /// Stop admission, wait for in-flight ticks until `timeout`, then abort
    /// the rest. Idempotent.
    pub async fn shutdown(&self, timeout: Duration) {
        let mut tasks = {
            let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
            self.root.cancel();
            std::mem::take(&mut *guard)
        };
        let drained = tokio::time::timeout(timeout, async {
            while let Some(finished) = tasks.join_next().await {
                log_join_result(finished);
            }
        })
        .await;
        if drained.is_err() {
            tracing::warn!(
                remaining = tasks.len(),
                timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                "background job drain deadline reached; aborting in-flight work"
            );
            tasks.shutdown().await;
        }
    }
}

/// How long process shutdown waits for in-flight ticks. The `backend` compose
/// service has no `stop_grace_period`, so Docker's 10s default applies.
pub const SHUTDOWN_DRAIN: Duration = Duration::from_secs(8);

static JOBS: LazyLock<JobRunner> = LazyLock::new(JobRunner::new);

/// The process-wide runner.
pub fn jobs() -> &'static JobRunner {
    &JOBS
}

/// Shut down the process-wide runner. See [`JobRunner::shutdown`].
pub async fn shutdown(timeout: Duration) {
    JOBS.shutdown(timeout).await;
}

fn log_join_result(result: Result<&'static str, tokio::task::JoinError>) {
    match result {
        Ok(name) => tracing::debug!(job = name, "background job stopped"),
        // Ticks catch their own panics; this is a panic in the loop itself.
        Err(error) if error.is_panic() => tracing::error!(%error, "background job loop panicked"),
        Err(_) => {}
    }
}

fn jitter_delay(jitter: Duration) -> Duration {
    let max = u64::try_from(jitter.as_millis()).unwrap_or(u64::MAX);
    if max == 0 {
        return Duration::ZERO;
    }
    Duration::from_millis(rand::random::<u64>() % (max.saturating_add(1)))
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

async fn run_periodic<F, Fut>(
    name: &'static str,
    cancel: CancellationToken,
    every: Every,
    mut work: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let start = tokio::time::Instant::now() + every.first_delay;
    let mut interval = tokio::time::interval_at(start, every.period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = interval.tick() => {}
        }
        let pause = jitter_delay(every.jitter);
        if !pause.is_zero() {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(pause) => {}
            }
        }
        // A cancel that raced with a ready tick must not admit more work.
        if cancel.is_cancelled() {
            return;
        }
        // The factory call happens inside the first poll, so it is covered too.
        let tick = AssertUnwindSafe(async { work().await }).catch_unwind();
        if let Err(panic) = tick.await {
            tracing::error!(
                job = name,
                panic = panic_message(panic.as_ref()),
                "background job tick panicked; resuming at next tick"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const FAST: Duration = Duration::from_millis(1);

    async fn wait_until(condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::time::sleep(FAST).await;
            }
        })
        .await
        .expect("condition not reached");
    }

    #[tokio::test]
    async fn shutdown_cancels_admission_and_awaits_in_flight_tick() {
        let runner = JobRunner::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let (entered_tx, entered) = tokio::sync::oneshot::channel::<()>();
        let entered_tx = Arc::new(Mutex::new(Some(entered_tx)));
        let (c, f) = (calls.clone(), finished.clone());
        runner.periodic("slow", Every::new(FAST), move || {
            let (c, f, entered_tx) = (c.clone(), f.clone(), entered_tx.clone());
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                if let Some(tx) = entered_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                f.store(true, Ordering::SeqCst);
            }
        });
        entered.await.unwrap();
        runner.shutdown(Duration::from_secs(5)).await;
        assert!(
            finished.load(Ordering::SeqCst),
            "in-flight tick was not awaited"
        );
        let after = calls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            after,
            "tick admitted after shutdown"
        );
        assert_eq!(after, 1);
    }

    #[tokio::test]
    async fn shutdown_aborts_stuck_tick_after_deadline() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let runner = JobRunner::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let (d, e) = (dropped.clone(), entered.clone());
        runner.periodic("stuck", Every::new(FAST), move || {
            let guard = Dropped(d.clone());
            let e = e.clone();
            async move {
                let _guard = guard;
                e.store(true, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
        });
        wait_until(|| entered.load(Ordering::SeqCst)).await;
        tokio::time::timeout(
            Duration::from_secs(2),
            runner.shutdown(Duration::from_millis(50)),
        )
        .await
        .expect("shutdown must be bounded");
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn panicking_tick_is_contained_and_job_resumes() {
        let runner = JobRunner::new();
        let panics = Arc::new(AtomicUsize::new(0));
        let healthy = Arc::new(AtomicUsize::new(0));
        let p = panics.clone();
        runner.periodic("panics", Every::new(FAST), move || {
            let p = p.clone();
            async move {
                p.fetch_add(1, Ordering::SeqCst);
                panic!("injected tick failure");
            }
        });
        let h = healthy.clone();
        runner.periodic("healthy", Every::new(FAST), move || {
            h.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        // The panicking job keeps ticking and does not take the other one down.
        wait_until(|| panics.load(Ordering::SeqCst) >= 3 && healthy.load(Ordering::SeqCst) >= 3)
            .await;
        runner.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn job_handle_stops_only_its_job() {
        let runner = JobRunner::new();
        let stopped = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicUsize::new(0));
        let s = stopped.clone();
        let handle = runner.periodic("stopped", Every::new(FAST), move || {
            s.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        let r = running.clone();
        runner.periodic("running", Every::new(FAST), move || {
            r.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        wait_until(|| stopped.load(Ordering::SeqCst) >= 1).await;
        handle.cancel();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let frozen = stopped.load(Ordering::SeqCst);
        let before = running.load(Ordering::SeqCst);
        wait_until(|| running.load(Ordering::SeqCst) > before + 2).await;
        assert_eq!(stopped.load(Ordering::SeqCst), frozen);
        runner.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn registration_after_shutdown_is_refused() {
        let runner = JobRunner::new();
        runner.shutdown(Duration::from_secs(1)).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let handle = runner.periodic("late", Every::new(FAST), move || {
            c.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        assert!(handle.is_cancelled());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn first_delay_and_jitter_hold_the_first_tick() {
        let runner = JobRunner::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        runner.periodic(
            "delayed",
            Every::new(FAST)
                .after(Duration::from_secs(3600))
                .jitter(Duration::from_millis(5)),
            move || {
                c.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            },
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        runner.shutdown(Duration::from_secs(1)).await;
        assert!(jitter_delay(Duration::from_millis(5)) <= Duration::from_millis(5));
        assert_eq!(jitter_delay(Duration::ZERO), Duration::ZERO);
    }
}
