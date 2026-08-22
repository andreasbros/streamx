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
        let player = match EmbeddedPlayer::launch(&PlayTarget::LocalFile(clip)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("launch failed: {e}");
                cx.quit();
                return;
            }
        };
        cx.spawn(async move |cx| {
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
