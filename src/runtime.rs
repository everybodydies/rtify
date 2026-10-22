//! The Tokio runtime that network, tray and portal work runs on, and the bridge back to the GTK
//! main loop. GTK objects never leave the main thread.

use std::future::Future;
use std::sync::OnceLock;

use gtk::glib;

pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rtify-io")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Run `fut` on Tokio and hand its output to `then` on the GTK main thread.
pub fn spawn_then<T, F>(fut: F, then: impl FnOnce(T) + 'static)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let (tx, rx) = async_channel::bounded(1);
    rt().spawn(async move {
        let _ = tx.send(fut.await).await;
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            then(v);
        }
    });
}

/// Forward every item from a Tokio-side channel to `f` on the main thread.
pub fn forward<T: 'static>(rx: async_channel::Receiver<T>, f: impl Fn(T) + 'static) {
    glib::spawn_future_local(async move {
        while let Ok(v) = rx.recv().await {
            f(v);
        }
    });
}
