//! Smoke test for the in-process libmpv player. Needs a display and a
//! clip: `STREAMX_TEST_CLIP=/path/clip.mp4 cargo test --test embedded_mpv -- --ignored`.

use std::time::{Duration, Instant};
use streamx_desktop::playback::embedded::EmbeddedPlayer;
use streamx_desktop::playback::PlayTarget;

#[test]
#[ignore]
fn libmpv_plays_local_file_in_own_window() {
    let Some(clip) = std::env::var_os("STREAMX_TEST_CLIP") else {
        eprintln!("SKIP: STREAMX_TEST_CLIP not set");
        return;
    };
    let player = EmbeddedPlayer::launch(&PlayTarget::LocalFile(clip.into())).expect("launch");
    let start = Instant::now();
    let mut snap = player.snapshot();
    while start.elapsed() < Duration::from_secs(10) && snap.time_pos <= 0.5 {
        std::thread::sleep(Duration::from_millis(250));
        snap = player.snapshot();
    }
    eprintln!(
        "vo_configured={} paused={} time_pos={:.2} duration={:.2} finished={}",
        player.vo_configured(),
        snap.paused,
        snap.time_pos,
        snap.duration,
        player.is_finished()
    );
    assert!(snap.duration > 0.0, "file loaded");
    assert!(snap.time_pos > 0.5, "playback advanced");
    player.seek(5.0, false).expect("seek");
    player.toggle_pause().expect("pause");
    std::thread::sleep(Duration::from_millis(500));
    assert!(player.snapshot().paused, "paused after toggle");
    player.stop();
}
