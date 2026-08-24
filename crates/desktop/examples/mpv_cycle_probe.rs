//! Stress probe: launch, play, dispose, and relaunch the embedded
//! player several times inside a GPUI app. A teardown deadlock or a
//! failed second start shows up as a hang or a missing PASS line.
//!
//!   STREAMX_TEST_CLIP=clip STREAMX_TEST_CLIP2=audio cargo run -p streamx-desktop --example mpv_cycle_probe

use gpui::Application;
use std::sync::Arc;
use std::time::Duration;
use streamx_desktop::playback::embedded::EmbeddedPlayer;
use streamx_desktop::playback::PlayTarget;

fn main() {
    let clips: Vec<std::path::PathBuf> = ["STREAMX_TEST_CLIP", "STREAMX_TEST_CLIP2"]
        .iter()
        .filter_map(|k| std::env::var_os(k).map(Into::into))
        .collect();
    Application::new().run(move |cx| {
        if clips.is_empty() {
            eprintln!("no clips configured");
            cx.quit();
            return;
        }
        let clips = clips.clone();
        cx.spawn(async move |cx| {
            for round in 0..4usize {
                let clip = clips[round % clips.len()].clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(EmbeddedPlayer::launch(&PlayTarget::LocalFile(clip)));
                });
                let player: Arc<EmbeddedPlayer> = loop {
                    match rx.try_recv() {
                        Ok(Ok(p)) => break Arc::new(p),
                        Ok(Err(e)) => {
                            println!("FAIL round {round}: launch: {e}");
                            cx.update(|cx| cx.quit());
                            return;
                        }
                        Err(_) => {
                            cx.background_executor()
                                .timer(Duration::from_millis(100))
                                .await
                        }
                    }
                };
                // Wait for playback to actually start.
                let mut started = false;
                for _ in 0..40 {
                    if player.snapshot().time_pos > 0.2 {
                        started = true;
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                }
                let vo = player.vo_configured();
                println!("round {round}: started={started} vo={vo}");
                if !started {
                    println!("FAIL round {round}: playback never started");
                    cx.update(|cx| cx.quit());
                    return;
                }
                // Dispose off-main, as the app does, and wait for it.
                let (dtx, drx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    player.stop();
                    drop(player);
                    let _ = dtx.send(());
                });
                let mut disposed = false;
                for _ in 0..40 {
                    if drx.try_recv().is_ok() {
                        disposed = true;
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                }
                if !disposed {
                    println!("FAIL round {round}: dispose hung");
                    cx.update(|cx| cx.quit());
                    return;
                }
            }
            println!("PASS: 4 launch/play/dispose cycles");
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
