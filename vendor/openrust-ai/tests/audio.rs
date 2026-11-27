use std::{error::Error, fs, time::Duration};

use openrust::{
    filesystem::Storage,
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode, UploadedFile},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{AiError, Audio, AudioFormat, ToAudio, Transcription, TranscriptionAudio};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

fn wav() -> Vec<u8> {
    let mut wav = b"RIFF\x28\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00".to_vec();
    wav.extend(b"\x80\x3e\x00\x00\x00\x7d\x00\x00\x02\x00\x10\x00data\x04\x00\x00\x00");
    wav.extend([0, 0, 5, 0]);
    wav
}

#[tokio::test]
async fn speech_and_transcription_use_binary_audio_and_report_usage()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let fixture = tokio::spawn(async move {
        for phase in ["speech", "text", "diarize", "invalid", "xai"] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            let path = match phase {
                "speech" => "/v1/audio/speech",
                "xai" => "/v1/stt",
                _ => "/v1/audio/transcriptions",
            };
            assert_eq!(line.trim_end(), format!("POST {path} HTTP/1.1"));
            let mut size = 0;
            let mut content_type = String::new();
            let mut authorized = false;
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (key, value) = line.split_once(':').unwrap();
                match key.to_ascii_lowercase().as_str() {
                    "content-length" => size = value.trim().parse().unwrap(),
                    "content-type" => content_type = value.trim().into(),
                    "authorization" => authorized = value.trim() == "Bearer audio-fixture",
                    _ => (),
                }
            }
            assert!(authorized);
            let mut payload = vec![0; size];
            socket.read_exact(&mut payload).await.unwrap();
            let (kind, body) = if phase == "speech" {
                let body: Value = serde_json::from_slice(&payload).unwrap();
                assert_eq!(content_type, "application/json");
                assert_eq!(
                    body,
                    json!({"model":"tts-default","input":"Read the notice","voice":"nova","response_format":"wav","instructions":"Speak clearly"})
                );
                ("audio/wav", wav())
            } else {
                assert!(content_type.starts_with("multipart/form-data; boundary="));
                let original = wav();
                assert!(payload.windows(original.len()).any(|part| part == original));
                let body = String::from_utf8_lossy(&payload);
                assert!(body.contains("name=\"file\"; filename=\"audio.wav\""));
                assert!(body.contains("Content-Type: audio/wav"));
                let response = if phase == "text" {
                    assert!(body.contains("\r\n\r\nen\r\n"));
                    assert!(body.contains("\r\n\r\ntranscript-default\r\n"));
                    assert!(body.contains("\r\n\r\njson\r\n"));
                    json!({"text":"Read the notice","usage":{"input_tokens":12,"output_tokens":3,"input_token_details":{"audio_tokens":11}}})
                } else if phase == "diarize" {
                    assert!(body.contains("\r\n\r\ndiarized_json\r\n"));
                    assert!(body.contains("\r\n\r\nauto\r\n"));
                    assert!(body.contains("\r\n\r\ndiarize-model\r\n"));
                    json!({"text":"Voice A: Read","segments":[{"start":0.0,"end":1.2,"text":"Read","speaker":"A"}],"usage":{"type":"duration","seconds":1.2}})
                } else if phase == "xai" {
                    assert!(!body.contains("name=\"response_format\""));
                    assert!(body.contains("\r\n\r\ngrok-voice-transcribe-2.0\r\n"));
                    let diarize = body.find("\r\n\r\ntrue\r\n").unwrap();
                    let file = body.find("name=\"file\"").unwrap();
                    assert!(diarize < file);
                    json!({"text":"Read","language":"en","duration":1.2,"words":[{"start":0.0,"end":1.0,"text":"Read","speaker":0}]})
                } else {
                    json!({"text":"invalid","segments":[{"start":2.0,"end":1.0,"text":"reversed"}]})
                };
                ("application/json", serde_json::to_vec(&response).unwrap())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.get_mut().write_all(head.as_bytes()).await.unwrap();
            socket.get_mut().write_all(&body).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_config(|base, _| {
                Ok([(
                    "filesystems".into(),
                    json!({"default":"local","disks":{
                        "local":{"driver":"local","root":base.join("private"),"throw":true},
                    }}),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/audio", || async {
                    assert!(matches!(Audio::of(" "), Err(AiError::InvalidPrompt)));
                    assert!(matches!(
                        Transcription::from_bytes(b"not audio".to_vec()),
                        Err(AiError::InvalidAudio)
                    ));
                    let reply = "Read the notice"
                        .to_audio()
                        .unwrap()
                        .female()
                        .format(AudioFormat::Wav)
                        .instructions("Speak clearly")
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(reply.content(), wav());
                    assert_eq!(reply.mime_type(), "audio/wav");
                    assert_eq!(reply.model, "tts-default");
                    assert_eq!(
                        TranscriptionAudio::try_from(&reply).unwrap().mime_type(),
                        "audio/wav"
                    );
                    let path = reply.store_as("audio", "notice.wav", None).await.unwrap();
                    assert_eq!(Storage::get(&path).await.unwrap().unwrap(), wav());
                    let transcript = Transcription::from_storage(&path, None)
                        .await
                        .unwrap()
                        .language("en")
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(transcript.text, "Read the notice");
                    assert_eq!(transcript.usage.audio_input_tokens, Some(11));
                    assert!(transcript.segments.is_empty());
                    let file = UploadedFile::from_bytes(
                        "fake.txt".into(),
                        Some("text/plain".into()),
                        wav().into(),
                    );
                    let transcript = Transcription::from_upload(&file)
                        .unwrap()
                        .diarize(true)
                        .generate_with(Some("openai"), Some("diarize-model"))
                        .await
                        .unwrap();
                    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("A"));
                    assert_eq!(transcript.usage.seconds, Some(1.2));
                    assert!(matches!(
                        Transcription::from_path(Storage::disk(None).unwrap().path(&path).unwrap())
                            .await
                            .unwrap()
                            .generate()
                            .await,
                        Err(AiError::InvalidResponse)
                    ));
                    let transcript = Transcription::from_upload(&file)
                        .unwrap()
                        .diarize(true)
                        .generate_with(Some("xai"), None)
                        .await
                        .unwrap();
                    assert_eq!(transcript.text, "Read");
                    assert_eq!(transcript.language.as_deref(), Some("en"));
                    assert_eq!(transcript.segments[0].speaker.as_deref(), Some("0"));
                    assert_eq!(transcript.usage.seconds, Some(1.2));
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Ok(())
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_AUDIO_PROVIDER=openai\nAI_TRANSCRIPTION_PROVIDER=openai\nOPENAI_AUDIO_MODEL=tts-default\nOPENAI_TRANSCRIPTION_MODEL=transcript-default\nOPENAI_URL=http://{address}/v1\nOPENAI_API_KEY=audio-fixture\nXAI_URL=http://{address}/v1\nXAI_API_KEY=audio-fixture\nXAI_TRANSCRIPTION_MODEL=grok-voice-transcribe-2.0\n"
        ),
    )?;
    assert_eq!(app.get("/audio").await?.status(), StatusCode::OK);
    fixture.await?;
    Ok(())
}

#[tokio::test]
async fn audio_and_transcription_fakes_record_requests_with_no_provider_credentials()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust_ai::{Ai, AudioResponse, AudioVoice};

    let create_app = || {
        TestApp::create(|builder| {
            builder
                .with_providers(openrust_ai::openrust_providers())
                .with_routing(Routing::default().web(|| {
                    Route::get("/fake-audio", || async {
                        assert!(!Audio::is_faked().unwrap());
                        assert!(!Transcription::is_faked().unwrap());
                        assert!(matches!(
                            Audio::assert_nothing_generated(),
                            Err(AiError::FakeNotInstalled)
                        ));
                        let audio = Audio::fake([AudioResponse::from_bytes(
                            wav(),
                            AudioFormat::Wav,
                            "fixture",
                            "audio-v1",
                        )
                        .unwrap()])
                        .unwrap();
                        audio.prevent_stray_audio(true);
                        Ai::assert_no_audio_generated().unwrap();
                        let speech = Audio::of("Read the notice")
                            .unwrap()
                            .format(AudioFormat::Wav)
                            .female()
                            .instructions("Slowly")
                            .generate_with(Some("isolated"), Some("explicit"))
                            .await
                            .unwrap();
                        assert_eq!(speech.content(), wav());
                        assert_eq!(speech.model, "audio-v1");
                        let error = Audio::of("Unexpected").unwrap().generate().await.err();
                        assert_eq!(error, Some(AiError::FakeExhausted));
                        Audio::assert_not_generated(|request| request.text == "Unexpected")
                            .unwrap();
                        Ai::assert_audio_generated(|request| {
                            request.provider.as_deref() == Some("isolated")
                                && request.model.as_deref() == Some("explicit")
                                && request.voice == AudioVoice::Female
                                && request.instructions.as_deref() == Some("Slowly")
                        })
                        .unwrap();
                        let transcript = Transcription::fake_with(|request| {
                            assert_eq!(request.audio.as_ref(), wav());
                            assert_eq!(request.mime_type, "audio/wav");
                            Ok("Recorded notice".into())
                        })
                        .unwrap();
                        let result =
                            Transcription::of(TranscriptionAudio::try_from(&speech).unwrap())
                                .language("en")
                                .diarize(true)
                                .generate()
                                .await
                                .unwrap();
                        assert_eq!(result.text, "Recorded notice");
                        assert_eq!(result.segments[0].speaker.as_deref(), Some("Speaker 1"));
                        Ai::assert_transcription_generated(|request| request.diarize).unwrap();
                        Transcription::assert_not_generated(|request| {
                            request.language.as_deref() == Some("fr")
                        })
                        .unwrap();
                        assert_eq!(transcript.recorded().len(), 1);
                        Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                    })?;
                    Ok(())
                }))
        })
    };
    let mut first = create_app()?;
    let mut second = create_app()?;
    assert_eq!(first.get("/fake-audio").await?.status(), StatusCode::OK);
    assert_eq!(second.get("/fake-audio").await?.status(), StatusCode::OK);
    Ok(())
}
