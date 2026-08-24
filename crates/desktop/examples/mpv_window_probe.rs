//! Probe: does libmpv create its own player window when hosted inside
//! a GPUI application (Cocoa main loop present)? Prints
//! `vo_configured=<bool>` after a few seconds and exits.
//!
//!   STREAMX_TEST_CLIP=clip.mp4 cargo run -p streamx-desktop --example mpv_window_probe

use gpui::Application;
use std::time::Duration;
use streamx_desktop::playback::embedded::EmbeddedPlayer;
use streamx_desktop::playback::PlayTarget;

fn main() {
    let clip = std::env::var_os("STREAMX_TEST_CLIP").map(std::path::PathBuf::from);
    Application::new().run(move |cx| {
        let Some(clip) = clip.clone() else {
            eprintln!("STREAMX_TEST_CLIP not set");
            cx.quit();
            return;
        };
        // Launch off the main thread: libmpv's macOS window creation
        // dispatches onto the main queue, so a main-thread launch can
        // deadlock waiting for itself.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(EmbeddedPlayer::launch(&PlayTarget::LocalFile(clip)));
        });
        cx.spawn(async move |cx| {
            let player = loop {
                match rx.try_recv() {
                    Ok(Ok(p)) => break p,
                    Ok(Err(e)) => {
                        eprintln!("launch failed: {e}");
                        cx.update(|cx| cx.quit());
                        return;
                    }
                    Err(_) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                    }
                }
            };
            for _ in 0..20 {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if player.vo_configured() {
                    break;
                }
            }
            let snap = player.snapshot();
            println!(
                "vo_configured={} time_pos={:.2} duration={:.2} finished={}",
                player.vo_configured(),
                snap.time_pos,
                snap.duration,
                player.is_finished()
            );
            player.stop();
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
