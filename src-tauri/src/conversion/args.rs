use std::path::{Path, PathBuf};

use crate::conversion::error::ConversionError;
use crate::conversion::types::{ConversionConfig, MetadataConfig, MetadataMode, VOLUME_EPSILON};
use crate::conversion::utils::{
    is_audio_only_container, is_nvenc_codec, is_videotoolbox_codec, map_nvenc_preset, parse_time,
};

pub fn build_ffmpeg_args(input: &str, output: &str, config: &ConversionConfig) -> Vec<String> {
    let mut args = Vec::new();

    if let Some(start) = &config.start_time {
        if !start.is_empty() {
            args.push("-ss".to_string());
            args.push(start.clone());
        }
    }

    args.push("-i".to_string());
    args.push(input.to_string());

    if let Some(end_str) = &config.end_time {
        if !end_str.is_empty() {
            if let Some(start_str) = &config.start_time {
                if !start_str.is_empty() {
                    if let (Some(start_t), Some(end_t)) =
                        (parse_time(start_str), parse_time(end_str))
                    {
                        let duration = end_t - start_t;
                        if duration > 0.0 {
                            args.push("-t".to_string());
                            args.push(format!("{:.3}", duration));
                        }
                    }
                } else {
                    args.push("-to".to_string());
                    args.push(end_str.clone());
                }
            } else {
                args.push("-to".to_string());
                args.push(end_str.clone());
            }
        }
    }

    match config.metadata.mode {
        MetadataMode::Clean => {
            args.push("-map_metadata".to_string());
            args.push("-1".to_string());
        }
        MetadataMode::Replace => {
            args.push("-map_metadata".to_string());
            args.push("-1".to_string());
            add_metadata_flags(&mut args, &config.metadata);
        }
        MetadataMode::Preserve => {
            add_metadata_flags(&mut args, &config.metadata);
        }
    }

    let is_audio_only = is_audio_only_container(&config.container);
    let is_nvenc = is_nvenc_codec(&config.video_codec);
    let is_videotoolbox = is_videotoolbox_codec(&config.video_codec);

    if is_audio_only {
        args.push("-vn".to_string());
    } else {
        args.push("-c:v".to_string());
        args.push(config.video_codec.clone());

        if config.video_bitrate_mode == "bitrate" {
            args.push("-b:v".to_string());
            args.push(format!("{}k", config.video_bitrate));
        } else if is_nvenc {
            let cq = (52.0 - (config.quality as f64 / 2.0))
                .round()
                .clamp(1.0, 51.0) as u32;
            args.push("-rc:v".to_string());
            args.push("vbr".to_string());
            args.push("-cq:v".to_string());
            args.push(cq.to_string());
        } else if is_videotoolbox {
            args.push("-q:v".to_string());
            args.push(config.quality.to_string());
        } else {
            args.push("-crf".to_string());
            args.push(config.crf.to_string());
        }

        if !is_videotoolbox {
            args.push("-preset".to_string());
            let preset_value = if is_nvenc {
                map_nvenc_preset(&config.preset)
            } else {
                config.preset.clone()
            };
            args.push(preset_value);
        }

        if is_nvenc {
            if config.nvenc_spatial_aq {
                args.push("-spatial_aq".to_string());
                args.push("1".to_string());
            }
            if config.nvenc_temporal_aq {
                args.push("-temporal_aq".to_string());
                args.push("1".to_string());
            }
        }

        if is_videotoolbox {
            if config.videotoolbox_allow_sw {
                args.push("-allow_sw".to_string());
                args.push("1".to_string());
            }
        }

        let mut video_filters = Vec::new();

        if config.flip_horizontal {
            video_filters.push("hflip".to_string());
        }

        if config.flip_vertical {
            video_filters.push("vflip".to_string());
        }

        match config.rotation.as_str() {
            "90" => video_filters.push("transpose=1".to_string()),
            "180" => video_filters.push("transpose=1,transpose=1".to_string()),
            "270" => video_filters.push("transpose=2".to_string()),
            _ => {}
        }

        if let Some(crop) = &config.crop {
            if crop.enabled {
                let crop_width = crop.width.max(1.0).round() as i32;
                let crop_height = crop.height.max(1.0).round() as i32;
                let crop_x = crop.x.max(0.0).round() as i32;
                let crop_y = crop.y.max(0.0).round() as i32;
                video_filters.push(format!(
                    "crop={}:{}:{}:{}",
                    crop_width, crop_height, crop_x, crop_y
                ));
            }
        }

        if let Some(burn_path) = &config.subtitle_burn_path {
            if !burn_path.is_empty() {
                let escaped_path = burn_path.replace('\\', "/").replace(':', "\\:");
                video_filters.push(format!("subtitles='{}'", escaped_path));
            }
        }

        if config.resolution != "original" || config.resolution == "custom" {
            let algorithm = match config.scaling_algorithm.as_str() {
                "lanczos" => ":flags=lanczos",
                "bilinear" => ":flags=bilinear",
                "nearest" => ":flags=neighbor",
                "bicubic" => ":flags=bicubic",
                _ => "",
            };

            let scale_filter = if config.resolution == "custom" {
                let w = config.custom_width.as_deref().unwrap_or("-1");
                let h = config.custom_height.as_deref().unwrap_or("-1");
                if w != "-1" && h != "-1" {
                    format!(
                        "scale={w}:{h}:force_original_aspect_ratio=decrease{algo},pad={w}:{h}:(ow-iw)/2:(oh-ih)/2",
                        w = w,
                        h = h,
                        algo = algorithm
                    )
                } else if w == "-1" && h == "-1" {
                    "scale=-1:-1".to_string()
                } else {
                    format!("scale={}:{}{}", w, h, algorithm)
                }
            } else {
                match config.resolution.as_str() {
                    "1080p" => format!("scale=-2:1080{}", algorithm),
                    "720p" => format!("scale=-2:720{}", algorithm),
                    "480p" => format!("scale=-2:480{}", algorithm),
                    _ => "scale=-1:-1".to_string(),
                }
            };

            video_filters.push(scale_filter);
        }

        if !video_filters.is_empty() {
            args.push("-vf".to_string());
            args.push(video_filters.join(","));
        }

        if config.fps != "original" {
            args.push("-r".to_string());
            args.push(config.fps.clone());
        }
    }

    if (!config.selected_audio_tracks.is_empty() || !config.selected_subtitle_tracks.is_empty())
        && !is_audio_only
    {
        args.push("-map".to_string());
        args.push("0:v:0".to_string());
    }

    if !config.selected_audio_tracks.is_empty() {
        for track_index in &config.selected_audio_tracks {
            args.push("-map".to_string());
            args.push(format!("0:{}", track_index));
        }
    }

    if !config.selected_audio_tracks.is_empty() {
        args.push("-c:a".to_string());
        args.push(config.audio_codec.clone());
    }

    if !config.selected_subtitle_tracks.is_empty() {
        for track_index in &config.selected_subtitle_tracks {
            args.push("-map".to_string());
            args.push(format!("0:{}", track_index));
        }
    } else if !is_audio_only {
        args.push("-map".to_string());
        args.push("0:s?".to_string());
    }

    if config.subtitle_burn_path.is_none()
        || config
            .subtitle_burn_path
            .as_ref()
            .map_or(true, |p| p.is_empty())
    {
        args.push("-c:s".to_string());
        args.push("copy".to_string());
    }

    let lossless_audio_codecs = ["flac", "alac", "pcm_s16le"];
    if !lossless_audio_codecs.contains(&config.audio_codec.as_str())
        && !config.selected_audio_tracks.is_empty()
    {
        args.push("-b:a".to_string());
        args.push(format!("{}k", config.audio_bitrate));
    }

    match config.audio_channels.as_str() {
        "stereo" => {
            args.push("-ac".to_string());
            args.push("2".to_string());
        }
        "mono" => {
            args.push("-ac".to_string());
            args.push("1".to_string());
        }
        _ => {}
    }

    let mut audio_filters: Vec<String> = Vec::new();

    if config.audio_normalize {
        audio_filters.push("loudnorm=I=-16:TP=-1.5:LRA=11".to_string());
    }

    if (config.audio_volume - 100.0).abs() > VOLUME_EPSILON {
        let volume_factor = config.audio_volume / 100.0;
        audio_filters.push(format!("volume={:.2}", volume_factor));
    }

    if !audio_filters.is_empty() {
        args.push("-af".to_string());
        args.push(audio_filters.join(","));
    }

    args.push("-y".to_string());
    args.push(output.to_string());

    args
}

fn add_metadata_flags(args: &mut Vec<String>, metadata: &MetadataConfig) {
    if let Some(v) = &metadata.title {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("title={}", v));
        }
    }
    if let Some(v) = &metadata.artist {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("artist={}", v));
        }
    }
    if let Some(v) = &metadata.album {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("album={}", v));
        }
    }
    if let Some(v) = &metadata.genre {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("genre={}", v));
        }
    }
    if let Some(v) = &metadata.date {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("date={}", v));
        }
    }
    if let Some(v) = &metadata.comment {
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("comment={}", v));
        }
    }
}

pub fn build_output_path(file_path: &str, container: &str, output_name: Option<String>) -> String {
    if let Some(custom) = output_name.and_then(|name| {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }) {
        let input_path = Path::new(file_path);
        let mut output: PathBuf = match input_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::new(),
        };
        output.push(custom);
        if output.extension().is_none() {
            output.set_extension(container);
        }
        output.to_string_lossy().to_string()
    } else {
        format!("{}_converted.{}", file_path, container)
    }
}

pub fn validate_task_input(
    file_path: &str,
    config: &ConversionConfig,
) -> Result<(), ConversionError> {
    let input_path = Path::new(file_path);
    if !input_path.exists() {
        return Err(ConversionError::InvalidInput(format!(
            "Input file does not exist: {}",
            file_path
        )));
    }
    if !input_path.is_file() {
        return Err(ConversionError::InvalidInput(format!(
            "Input path is not a file: {}",
            file_path
        )));
    }

    if config.resolution == "custom" {
        let w_str = config.custom_width.as_deref().unwrap_or("-1");
        let h_str = config.custom_height.as_deref().unwrap_or("-1");

        let w = w_str.parse::<i32>().map_err(|_| {
            ConversionError::InvalidInput(format!("Invalid custom width: {}", w_str))
        })?;
        let h = h_str.parse::<i32>().map_err(|_| {
            ConversionError::InvalidInput(format!("Invalid custom height: {}", h_str))
        })?;

        if w == 0 || h == 0 {
            return Err(ConversionError::InvalidInput(
                "Resolution dimensions cannot be zero".to_string(),
            ));
        }
        if w < -1 || h < -1 {
            return Err(ConversionError::InvalidInput(
                "Resolution dimensions cannot be negative (except -1 for auto)".to_string(),
            ));
        }
    }

    if config.video_bitrate_mode == "bitrate" && !is_audio_only_container(&config.container) {
        let bitrate = config.video_bitrate.parse::<f64>().map_err(|_| {
            ConversionError::InvalidInput(format!(
                "Invalid video bitrate: {}",
                config.video_bitrate
            ))
        })?;
        if bitrate <= 0.0 {
            return Err(ConversionError::InvalidInput(
                "Video bitrate must be positive".to_string(),
            ));
        }
    }

    Ok(())
}
