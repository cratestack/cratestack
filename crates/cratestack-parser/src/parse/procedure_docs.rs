use std::collections::BTreeMap;

pub(super) fn split_procedure_docs(
    docs: Vec<String>,
) -> (Vec<String>, BTreeMap<String, Vec<String>>) {
    let mut procedure_docs = Vec::new();
    let mut arg_docs = BTreeMap::<String, Vec<String>>::new();

    for doc in docs {
        if let Some(param) = doc.strip_prefix("@param ") {
            let mut parts = param.trim().splitn(2, char::is_whitespace);
            let Some(name) = parts.next() else {
                continue;
            };
            let description = parts.next().unwrap_or_default().trim();
            if description.is_empty() {
                continue;
            }
            arg_docs
                .entry(name.to_owned())
                .or_default()
                .push(description.to_owned());
        } else {
            procedure_docs.push(doc);
        }
    }

    (procedure_docs, arg_docs)
}
