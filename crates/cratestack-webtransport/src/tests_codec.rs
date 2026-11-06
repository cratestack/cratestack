use super::*;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame};

fn split_feed(decoder: &mut FrameDecoder, bytes: &[u8], step: usize) -> Vec<Vec<u8>> {
    bytes
        .chunks(step)
        .flat_map(|chunk| decoder.feed(chunk).expect("feed"))
        .collect()
}

#[test]
fn frames_survive_arbitrary_chunking_in_both_codecs() {
    for codec in [WireCodec::Cbor, WireCodec::Json] {
        let frames = [
            ClientFrame::init(Default::default()),
            ClientFrame::request(
                7,
                "procedure.roomMessages",
                serde_json::json!({"roomId": "r\n1"}),
            ),
        ];
        let wire: Vec<u8> = frames
            .iter()
            .flat_map(|f| codec.encode(f).unwrap())
            .collect();
        for step in [1, 2, 3, 7, wire.len()] {
            let mut decoder = FrameDecoder::new(codec, 1024);
            let decoded: Vec<ClientFrame> = split_feed(&mut decoder, &wire, step)
                .iter()
                .map(|bytes| codec.decode(bytes).unwrap())
                .collect();
            assert_eq!(decoded, frames, "{codec:?} split every {step} bytes");
            assert_eq!(decoder.pending_len(), 0);
        }
    }
}

#[test]
fn a_newline_inside_a_json_string_does_not_end_the_frame() {
    let bytes = WireCodec::Json
        .encode(&ClientFrame::request(1, "op", serde_json::json!("a\nb")))
        .unwrap();
    assert_eq!(
        bytes.iter().filter(|b| **b == b'\n').count(),
        1,
        "only the delimiter"
    );
}

#[test]
fn an_oversized_frame_is_refused_before_it_completes() {
    for codec in [WireCodec::Cbor, WireCodec::Json] {
        let big = codec
            .encode(&ClientFrame::request(
                1,
                "op",
                serde_json::json!("x".repeat(200)),
            ))
            .unwrap();
        let mut decoder = FrameDecoder::new(codec, 64);
        // Half the frame is already over the limit, so the refusal comes
        // before the end of the frame ever arrives.
        let error = decoder.feed(&big[..big.len() / 2]).unwrap_err();
        assert!(
            matches!(error, CratestackError::BadRequest(_)),
            "{codec:?}: {error:?}"
        );
    }
}

#[test]
fn codec_selection_prefers_the_subprotocol_then_the_query() {
    let offered = vec![
        "other".to_owned(),
        JSON_PROTOCOL.to_owned(),
        CBOR_PROTOCOL.to_owned(),
    ];
    assert_eq!(
        WireCodec::select(&offered, Some("codec=cbor")).unwrap(),
        (WireCodec::Json, true)
    );
    assert_eq!(
        WireCodec::select(&[], Some("x=1&codec=json")).unwrap(),
        (WireCodec::Json, false)
    );
    assert_eq!(
        WireCodec::select(&[], None).unwrap(),
        (WireCodec::Cbor, false)
    );
    assert!(WireCodec::select(&[], Some("codec=xml")).is_err());
}

#[test]
fn a_null_item_is_still_an_item() {
    for codec in [WireCodec::Cbor, WireCodec::Json] {
        let bytes = codec
            .encode(&ServerFrame::next(3, serde_json::Value::Null))
            .unwrap();
        let mut decoder = FrameDecoder::new(codec, 1024);
        let frame: ServerFrame = codec.decode(&decoder.feed(&bytes).unwrap()[0]).unwrap();
        assert_eq!(frame.next, Some(serde_json::Value::Null), "{codec:?}");
        assert!(frame.end.is_none() && frame.ready.is_none() && frame.err.is_none());
    }
}

#[test]
fn client_frames_classify_by_the_key_present() {
    assert!(matches!(
        ClientFrame::init(Default::default()).classify(),
        Ok(ClientMessage::Init(_))
    ));
    let request = ClientFrame {
        id: 4,
        init: None,
        op: Some("op".into()),
        input: None,
        cancel: None,
    };
    assert_eq!(
        request.classify().unwrap(),
        ClientMessage::Request {
            id: 4,
            op: "op".into(),
            input: serde_json::Value::Null
        }
    );
    let both = ClientFrame {
        id: 0,
        init: Some(Default::default()),
        op: Some("op".into()),
        input: None,
        cancel: None,
    };
    assert!(both.classify().is_err());
    let late_init = ClientFrame {
        id: 9,
        ..ClientFrame::init(Default::default())
    };
    assert!(late_init.classify().is_err());
    let neither = ClientFrame {
        id: 1,
        init: None,
        op: None,
        input: None,
        cancel: None,
    };
    assert!(neither.classify().is_err());
    assert_eq!(
        ClientFrame::cancel(3).classify().unwrap(),
        ClientMessage::Cancel { id: 3 }
    );
    let cancel_and_op = ClientFrame {
        cancel: Some(Default::default()),
        ..ClientFrame::request(3, "op", serde_json::Value::Null)
    };
    assert!(cancel_and_op.classify().is_err());
}
