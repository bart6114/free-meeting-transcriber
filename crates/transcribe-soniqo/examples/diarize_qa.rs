//! Manual QA harness for the FluidAudio diarizer bridge.
//!
//! Usage:
//!   cargo run -p transcribe-soniqo --example diarize_qa -- state
//!   cargo run -p transcribe-soniqo --example diarize_qa -- download
//!   cargo run -p transcribe-soniqo --example diarize_qa -- run <raw-f32le-16k-mono-file>

use transcribe_soniqo::diarize;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("state") => {
            println!("{:?}", diarize::model_download_state());
        }
        Some("download") => {
            diarize::start_model_download().expect("start download");
            loop {
                let state = diarize::model_download_state().expect("state");
                println!("{state:?}");
                if state.status == "ready" || state.status == "error" {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
        Some("run") => {
            let path = args.get(2).expect("path to raw f32le file");
            let bytes = std::fs::read(path).expect("read samples");
            let samples: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            println!(
                "samples: {} ({:.1}s), ready: {}",
                samples.len(),
                samples.len() as f64 / 16000.0,
                diarize::is_ready()
            );
            let started = std::time::Instant::now();
            let segments = diarize::diarize_samples(&samples, 16000).expect("diarize");
            let elapsed = started.elapsed();
            let mut speakers: Vec<i32> = segments.iter().map(|s| s.speaker_index).collect();
            speakers.sort();
            speakers.dedup();
            println!(
                "{} segments, {} distinct speakers {:?}, wall {:.1}s",
                segments.len(),
                speakers.len(),
                speakers,
                elapsed.as_secs_f64()
            );
            for s in segments.iter().take(20) {
                println!(
                    "  {:>7.1}s - {:>7.1}s  speaker {}",
                    s.start_ms as f64 / 1000.0,
                    s.end_ms as f64 / 1000.0,
                    s.speaker_index
                );
            }
        }
        _ => eprintln!("usage: diarize_qa state|download|run <file>"),
    }
}
