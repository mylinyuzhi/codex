use super::*;

#[test]
fn test_generate_speech_options() {
    let options = GenerateSpeechOptions::new("tts-1", "Hello")
        .with_voice("alloy")
        .with_speed(1.0);

    assert!(options.model.is_string());
    assert_eq!(options.text, "Hello");
    assert_eq!(
        options.voice.as_ref().map(|v| &v.id),
        Some(&"alloy".to_string())
    );
    assert_eq!(options.speed, Some(1.0));
}

#[test]
fn test_speech_voice() {
    let voice = SpeechVoice::new("alloy").with_name("Alloy");
    assert_eq!(voice.id, "alloy");
    assert_eq!(voice.name, Some("Alloy".to_string()));
}

#[test]
fn test_speech_format() {
    assert_eq!(SpeechFormat::Mp3.media_type(), "audio/mpeg");
    assert_eq!(SpeechFormat::Wav.media_type(), "audio/wav");
    assert_eq!(SpeechFormat::default(), SpeechFormat::Mp3);
}

#[test]
fn test_generated_audio_file() {
    let audio = GeneratedAudioFile::mp3(vec![0u8, 1, 2, 3]);
    assert_eq!(audio.media_type, "audio/mpeg");
    assert_eq!(audio.extension(), "mp3");
    assert!(!audio.data.is_empty());
}
