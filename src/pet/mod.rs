pub mod config;

#[cfg(target_os = "macos")]
mod native {
    use anyhow::Context;
    use std::{
        ffi::c_void,
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    struct State {
        shutdown: Arc<AtomicBool>,
        server_done: Arc<AtomicBool>,
        ui_ready: Arc<AtomicBool>,
    }

    unsafe extern "C" {
        fn lai_pet_run(
            ui_port: u16,
            api_port: u16,
            context: *mut c_void,
            should_stop: extern "C" fn(*mut c_void) -> bool,
            server_done: extern "C" fn(*mut c_void) -> bool,
            request_stop: extern "C" fn(*mut c_void),
            mark_ready: extern "C" fn(*mut c_void),
        ) -> i32;
    }

    extern "C" fn should_stop(context: *mut c_void) -> bool {
        unsafe { &*(context.cast::<State>()) }
            .shutdown
            .load(Ordering::Acquire)
    }
    extern "C" fn is_server_done(context: *mut c_void) -> bool {
        unsafe { &*(context.cast::<State>()) }
            .server_done
            .load(Ordering::Acquire)
    }
    extern "C" fn request_stop(context: *mut c_void) {
        unsafe { &*(context.cast::<State>()) }
            .shutdown
            .store(true, Ordering::Release);
    }
    extern "C" fn mark_ready(context: *mut c_void) {
        unsafe { &*(context.cast::<State>()) }
            .ui_ready
            .store(true, Ordering::Release);
    }

    pub fn run(
        port: u16,
        shutdown: Arc<AtomicBool>,
        server_done: Arc<AtomicBool>,
        ui_ready: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        let config = super::config::load()?;
        let assets = lai::pet::assets::AssetStore::new(
            config.model,
            serde_json::to_vec(&config.animations)?,
        );
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).context("cannot start pet asset server")?;
        let ui_port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let finished = Arc::new(AtomicBool::new(false));
        let end = Arc::clone(&finished);
        let worker = thread::spawn(move || {
            while !end.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let mut data = [0; 4096];
                        if let Ok(size) = stream.read(&mut data) {
                            let line = String::from_utf8_lossy(&data[..size]);
                            let mut parts = line.lines().next().unwrap_or("").split_whitespace();
                            let found = match (parts.next(), parts.next()) {
                                (Some("GET"), Some(path)) => assets.get(path),
                                _ => None,
                            };
                            let (status, mime, body) = match found {
                                Some((mime, body)) => ("200 OK", mime, body),
                                None => ("404 Not Found", "text/plain", &b"Not found"[..]),
                            };
                            let header = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream
                                .write_all(header.as_bytes())
                                .and_then(|_| stream.write_all(body));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => {
                        log::error!("Pet asset server failed: {error}");
                        break;
                    }
                }
            }
        });
        let mut state = State {
            shutdown,
            server_done,
            ui_ready,
        };
        // The native event loop runs synchronously on the main thread; callbacks do not outlive state.
        let status = unsafe {
            lai_pet_run(
                ui_port,
                port,
                (&mut state as *mut State).cast(),
                should_stop,
                is_server_done,
                request_stop,
                mark_ready,
            )
        };
        finished.store(true, Ordering::Release);
        let _ = worker.join();
        anyhow::ensure!(status == 0, "could not open the desktop pet");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use native::run;
