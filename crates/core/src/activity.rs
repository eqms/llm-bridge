//! Erkennt, ob der Proxy gerade Anfragen bearbeitet – damit ein automatisches Update
//! den Dienst nur neu startet, wenn keine Antwort abgebrochen wird.

use axum::body::{Body, Bytes, HttpBody};
use http_body::{Frame, SizeHint};
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::stats::now_ms;

#[derive(Debug)]
pub struct Activity {
    in_flight: AtomicUsize,
    /// Zeitpunkt (ms seit Epoche), an dem zuletzt eine Anfrage begann oder endete.
    last_ms: AtomicI64,
}

impl Default for Activity {
    fn default() -> Self {
        Self { in_flight: AtomicUsize::new(0), last_ms: AtomicI64::new(now_ms()) }
    }
}

impl Activity {
    /// Markiert eine laufende Anfrage, bis der zurückgegebene Wächter fällt.
    pub fn begin(self: &Arc<Self>) -> ActivityGuard {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        self.last_ms.store(now_ms(), Ordering::SeqCst);
        ActivityGuard(self.clone())
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Wahr, wenn keine Anfrage läuft und die letzte mindestens `quiet` zurückliegt.
    pub fn is_idle(&self, quiet: Duration) -> bool {
        self.in_flight() == 0
            && now_ms().saturating_sub(self.last_ms.load(Ordering::SeqCst)) >= quiet.as_millis() as i64
    }
}

#[derive(Debug)]
pub struct ActivityGuard(Arc<Activity>);

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        self.0.last_ms.store(now_ms(), Ordering::SeqCst);
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Antwort-Body, der den Wächter hält, bis er vollständig gesendet oder verworfen ist –
/// bei Streams also bis zum letzten Chunk, nicht nur bis der Handler zurückkehrt.
pub(crate) struct TrackedBody {
    inner: Body,
    _guard: ActivityGuard,
}

impl TrackedBody {
    pub(crate) fn wrap(inner: Body, guard: ActivityGuard) -> Body {
        Body::new(Self { inner, _guard: guard })
    }
}

impl HttpBody for TrackedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_only_without_requests_and_after_quiet_period() {
        let a = Arc::new(Activity::default());
        assert!(a.is_idle(Duration::ZERO));
        let g = a.begin();
        assert_eq!(a.in_flight(), 1);
        assert!(!a.is_idle(Duration::ZERO));
        drop(g);
        assert!(a.is_idle(Duration::ZERO));
        assert!(!a.is_idle(Duration::from_secs(60)));
    }
}
