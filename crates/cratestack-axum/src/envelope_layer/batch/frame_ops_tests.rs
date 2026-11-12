//! cratestack#1352: a frame whose `input` holds a CBOR byte string (a `Uuid`
//! or `Bytes` field, as the TypeScript client writes them) must not stop the
//! layer reading the frame's op. `FrameOp` skips `input` with `IgnoredAny`,
//! which accepts any CBOR item.

use super::{PAYLOAD_MEDIA_TYPE, frame_ops};

#[test]
fn frame_ops_reads_the_op_of_a_frame_whose_input_holds_a_byte_string() {
    let mut body = Vec::new();
    let mut e = minicbor::Encoder::new(&mut body);
    e.array(2).expect("array");
    e.map(3).expect("frame map");
    e.str("id").expect("k").u64(0).expect("v");
    e.str("op").expect("k").str("procedure.echo").expect("v");
    e.str("input").expect("k");
    e.map(1).expect("input map");
    e.str("data").expect("k").bytes(&[0, 1, 2, 255]).expect("v");
    e.map(2).expect("frame map");
    e.str("id").expect("k").u64(1).expect("v");
    e.str("op").expect("k").str("model.User.list").expect("v");

    assert_eq!(
        frame_ops(Some(PAYLOAD_MEDIA_TYPE), &body),
        Some(vec![
            "procedure.echo".to_owned(),
            "model.User.list".to_owned()
        ])
    );
}
