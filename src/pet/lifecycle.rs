use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub fn wait_for_pet_ui(ready: &AtomicBool, shutdown: &AtomicBool) -> bool {
    while !ready.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    !shutdown.load(Ordering::Acquire)
}

struct ServerExit(Arc<AtomicBool>, Arc<AtomicBool>);

impl Drop for ServerExit {
    fn drop(&mut self) {
        self.1.store(true, Ordering::Release);
        self.0.store(true, Ordering::Release);
    }
}

pub fn run_pet_lifecycle(
    serve: impl FnOnce(Arc<AtomicBool>) -> anyhow::Result<()> + Send + 'static,
    ui: impl FnOnce(Arc<AtomicBool>, Arc<AtomicBool>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_done = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let worker_done = Arc::clone(&server_done);
    let worker = std::thread::spawn(move || {
        let _exit = ServerExit(Arc::clone(&worker_shutdown), worker_done);
        serve(worker_shutdown)
    });
    let ui_result = ui(Arc::clone(&shutdown), server_done);
    shutdown.store(true, Ordering::Release);
    let server_result = worker
        .join()
        .map_err(|_| anyhow::anyhow!("LAI server thread panicked"))?;
    ui_result?;
    server_result
}

pub fn detached_arguments(port: u16, pet: bool) -> Vec<String> {
    let mut args = vec![
        "serve".into(),
        "--port".into(),
        port.to_string(),
        "--ready-signal".into(),
    ];
    if pet {
        args.push("--pet".into());
    }
    args
}
