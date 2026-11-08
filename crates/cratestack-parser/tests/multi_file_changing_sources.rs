//! A file that changes between two reads of one load is an error, never a
//! panic (docs/design/multi-file-schemas.md section 9).

use std::cell::Cell;
use std::io;
use std::path::Path;

use cratestack_parser::{LoadOptions, SchemaSources, load_schema};

const PART: &str = "/fixture/models/p.cstack";

struct Racy {
    part_reads: Cell<u32>,
}

impl SchemaSources for Racy {
    fn read(&self, path: &Path) -> io::Result<String> {
        if path == Path::new("/fixture/schema.cstack") {
            return Ok("model R {\n  id Int @id\n}\n".to_owned());
        }
        let reads = self.part_reads.get();
        self.part_reads.set(reads + 1);
        // The first read declares `part of`; the re-read, after the root
        // refused to list it, does not.
        Ok(if reads == 0 {
            "part of \"../schema.cstack\"\n\nmodel P {\n  id Int @id\n}\n".to_owned()
        } else {
            "model P {\n  id Int @id\n}\n".to_owned()
        })
    }
}

#[test]
fn a_part_edited_away_mid_load_is_an_error_not_a_panic() {
    let sources = Racy {
        part_reads: Cell::new(0),
    };
    let options = LoadOptions {
        follow_part_of: true,
        sources: Some(&sources),
    };
    let error = load_schema(PART, &options).unwrap_err();
    assert!(
        error
            .message()
            .contains("changed while it was being loaded"),
        "{}",
        error.message()
    );
}
