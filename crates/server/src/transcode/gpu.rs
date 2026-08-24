use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwAccel {
    Nvenc,
    Vaapi,
    Qsv,
    VideoToolbox,
    None,
}

pub async fn detect_hardware() -> HwAccel {
    let available = match query_ffmpeg_hwaccels().await {
        Ok(list) => list,
        Err(_) => return HwAccel::None,
    };

    if available.contains(&"cuda".to_string()) && nvidia_gpu_present().await {
        return HwAccel::Nvenc;
    }

    if available.contains(&"vaapi".to_string()) && vaapi_device_exists() {
        return HwAccel::Vaapi;
    }

    if available.contains(&"qsv".to_string()) {
        return HwAccel::Qsv;
    }

    if available.contains(&"videotoolbox".to_string()) {
        return HwAccel::VideoToolbox;
    }

    HwAccel::None
}

pub fn encoder_for_hw(hw: &HwAccel) -> &'static str {
    match hw {
        HwAccel::Nvenc => "h264_nvenc",
        HwAccel::Vaapi => "h264_vaapi",
        HwAccel::Qsv => "h264_qsv",
        HwAccel::VideoToolbox => "h264_videotoolbox",
        HwAccel::None => "libx264",
    }
}

pub fn hw_decode_flags(hw: &HwAccel) -> Vec<String> {
    match hw {
        HwAccel::Nvenc => vec![
            "-hwaccel".into(),
            "cuda".into(),
            "-hwaccel_output_format".into(),
            "cuda".into(),
        ],
        HwAccel::Vaapi => vec![
            "-hwaccel".into(),
            "vaapi".into(),
            "-hwaccel_device".into(),
            "/dev/dri/renderD128".into(),
            "-hwaccel_output_format".into(),
            "vaapi".into(),
        ],
        HwAccel::Qsv => vec!["-hwaccel".into(), "qsv".into()],
        HwAccel::VideoToolbox => vec!["-hwaccel".into(), "videotoolbox".into()],
        HwAccel::None => vec![],
    }
}

async fn query_ffmpeg_hwaccels() -> std::result::Result<Vec<String>, ()> {
    let output = Command::new(crate::ffmpeg_bin::ffmpeg())
        .args(["-hide_banner", "-hwaccels"])
        .output()
        .await
        .map_err(|_| ())?;

    if !output.status.success() {
        return Err(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let accels: Vec<String> = stdout
        .lines()
        .skip(1)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(accels)
}

async fn nvidia_gpu_present() -> bool {
    Command::new("nvidia-smi")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn vaapi_device_exists() -> bool {
    std::path::Path::new("/dev/dri/renderD128").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_selection() {
        assert_eq!(encoder_for_hw(&HwAccel::Nvenc), "h264_nvenc");
        assert_eq!(encoder_for_hw(&HwAccel::Vaapi), "h264_vaapi");
        assert_eq!(encoder_for_hw(&HwAccel::Qsv), "h264_qsv");
        assert_eq!(encoder_for_hw(&HwAccel::VideoToolbox), "h264_videotoolbox");
        assert_eq!(encoder_for_hw(&HwAccel::None), "libx264");
    }

    #[test]
    fn decode_flags_cpu_empty() {
        assert!(hw_decode_flags(&HwAccel::None).is_empty());
    }

    #[test]
    fn decode_flags_nvenc() {
        let flags = hw_decode_flags(&HwAccel::Nvenc);
        assert_eq!(flags.len(), 4);
        assert_eq!(flags[0], "-hwaccel");
        assert_eq!(flags[1], "cuda");
    }
}
