use super::*;

#[test]
fn test_transcribe_options() {
    let options =
        TranscribeOptions::new("whisper-1", AudioData::bytes(vec![0, 1, 2])).with_language("en");

    assert!(options.model.is_string());
    assert_eq!(options.language, Some("en".to_string()));
}

#[test]
fn test_audio_data() {
    let bytes = AudioData::bytes(vec![0, 1, 2, 3]);
    match bytes {
        AudioData::Bytes(data) => assert_eq!(data.len(), 4),
        _ => panic!("Expected Bytes variant"),
    }

    let url = AudioData::url("https://example.com/audio.mp3");
    match url {
        AudioData::Url(u) => assert_eq!(u, "https://example.com/audio.mp3"),
        _ => panic!("Expected Url variant"),
    }
}

#[test]
fn test_transcription_segment() {
    let segment = TranscriptionSegment::new(0, 0.0, 2.5, "Hello world");
    assert_eq!(segment.id, 0);
    assert_eq!(segment.duration(), 2.5);
}

#[test]
fn test_detect_audio_content_type() {
    // MP3 with ID3 tag
    assert_eq!(detect_audio_content_type(b"ID3\x00\x00\x00"), "audio/mpeg");

    // WAV
    assert_eq!(
        detect_audio_content_type(b"RIFF\x00\x00\x00\x00WAVE"),
        "audio/wav"
    );

    // OGG
    assert_eq!(detect_audio_content_type(b"OggS\x00"), "audio/ogg");

    // FLAC
    assert_eq!(detect_audio_content_type(b"fLaC\x00"), "audio/flac");
}
