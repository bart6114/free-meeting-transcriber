use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use owhisper_client::LocalSoniqoLiveClient;
use owhisper_interface::stream::StreamResponse;
use owhisper_interface::{ControlMessage, MixedMessage};

fn read_f32_wav(path: &str) -> (u32, Vec<f32>) {
    let data = std::fs::read(path).expect("read wav");
    assert_eq!(&data[0..4], b"RIFF");
    let mut pos = 12usize;
    let mut sample_rate = 0u32;
    let mut bits = 0u16;
    let mut samples = Vec::new();
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &data[pos + 8..(pos + 8 + size).min(data.len())];
        if id == b"fmt " {
            sample_rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
        } else if id == b"data" {
            samples = match bits {
                32 => body
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
                16 => body
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f32 / i16::MAX as f32)
                    .collect(),
                other => panic!("unsupported bits: {other}"),
            };
        }
        pos += 8 + size + (size & 1);
    }
    (sample_rate, samples)
}

fn f32_to_i16_bytes(samples: &[f32]) -> Bytes {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Bytes::from(out)
}

async fn run_live(samples: &[f32], gain: f32, pace: bool) -> Vec<(String, bool, f64, f64)> {
    let model = hypr_transcribe_soniqo::SoniqoModel::ParakeetStreaming;
    let client = LocalSoniqoLiveClient::new(model);

    let (tx, rx) = tokio::sync::mpsc::channel::<MixedMessage<(Bytes, Bytes), ControlMessage>>(1024);
    let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
    let (mut stream, _handle) = client
        .from_realtime_audio_dual(outbound)
        .await
        .expect("start live");

    let chunk = 1600usize;
    let scaled: Vec<f32> = samples.iter().map(|s| s * gain).collect();
    let feeder = tokio::spawn(async move {
        for c in scaled.chunks(chunk) {
            let mic = f32_to_i16_bytes(c);
            let spk = Bytes::from(vec![0u8; c.len() * 2]);
            tx.send(MixedMessage::Audio((mic, spk))).await.ok();
            if pace {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        tx.send(MixedMessage::Control(ControlMessage::CloseStream))
            .await
            .ok();
    });

    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamResponse::TranscriptResponse {
                start,
                duration,
                is_final,
                channel,
                channel_index,
                ..
            }) => {
                if channel_index.first() != Some(&0) {
                    continue;
                }
                let text = channel
                    .alternatives
                    .first()
                    .map(|a| a.transcript.clone())
                    .unwrap_or_default();
                out.push((text, is_final, start, duration));
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("stream error: {e}");
                break;
            }
        }
    }
    feeder.await.ok();
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let wav_path = args.get(1).expect("usage: soniqo_live_repro <wav> [gain]");
    let gain: f32 = args.get(2).map(|g| g.parse().unwrap()).unwrap_or(1.0);
    let pace = std::env::var("PACE").is_ok();

    let (rate, samples) = read_f32_wav(wav_path);
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    println!(
        "wav rate={rate} samples={} dur={:.2}s peak={peak:.4} rms={rms:.5} gain={gain}",
        samples.len(),
        samples.len() as f64 / rate as f64
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let responses = rt.block_on(run_live(&samples, gain, pace));
    println!("--- live responses (mic channel): {}", responses.len());
    for (text, is_final, start, duration) in &responses {
        println!(
            "[{}] start={start:.2} dur={duration:.2} text={text:?}",
            if *is_final { "FINAL" } else { "part " }
        );
    }

    let batch = hypr_transcribe_soniqo::transcribe_file(
        hypr_transcribe_soniqo::SoniqoModel::ParakeetBatch,
        wav_path,
        None,
    );
    println!("--- batch reference: {:?}", batch.map(|t| t.text));

    let streaming_oneshot = hypr_transcribe_soniqo::transcribe_file(
        hypr_transcribe_soniqo::SoniqoModel::ParakeetStreaming,
        wav_path,
        None,
    );
    println!(
        "--- streaming model one-shot: {:?}",
        streaming_oneshot.map(|t| t.text)
    );
}
