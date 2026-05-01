use quote::quote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PolicyAst {
    Term(String),
    And(Vec<PolicyAst>),
    Or(Vec<PolicyAst>),
}

pub(super) fn parse_policy_ast(expression: &str) -> Result<PolicyAst, String> {
    PolicyExpressionParser::new(expression).parse()
}

pub(super) fn generate_policy_ast_tokens<TermFn>(
    ast: &PolicyAst,
    term_fn: &TermFn,
    and_path: proc_macro2::TokenStream,
    or_path: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String>
where
    TermFn: Fn(&str) -> Result<proc_macro2::TokenStream, String>,
{
    match ast {
        PolicyAst::Term(term) => term_fn(term),
        PolicyAst::And(parts) => {
            let generated = parts
                .iter()
                .map(|part| {
                    generate_policy_ast_tokens(part, term_fn, and_path.clone(), or_path.clone())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { #and_path(&[#(#generated),*]) })
        }
        PolicyAst::Or(parts) => {
            let generated = parts
                .iter()
                .map(|part| {
                    generate_policy_ast_tokens(part, term_fn, and_path.clone(), or_path.clone())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { #or_path(&[#(#generated),*]) })
        }
    }
}

struct PolicyExpressionParser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> PolicyExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn parse(mut self) -> Result<PolicyAst, String> {
        let expr = self.parse_or()?;
        self.skip_whitespace();
        if !self.is_eof() {
            return Err(format!(
                "unexpected trailing policy expression near '{}'",
                &self.input[self.cursor..]
            ));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<PolicyAst, String> {
        let mut nodes = vec![self.parse_and()?];
        loop {
            self.skip_whitespace();
            if !self.consume_str("||") {
                break;
            }
            nodes.push(self.parse_and()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("or node should exist")
        } else {
            PolicyAst::Or(nodes)
        })
    }

    fn parse_and(&mut self) -> Result<PolicyAst, String> {
        let mut nodes = vec![self.parse_factor()?];
        loop {
            self.skip_whitespace();
            if !self.consume_str("&&") {
                break;
            }
            nodes.push(self.parse_factor()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("and node should exist")
        } else {
            PolicyAst::And(nodes)
        })
    }

    fn parse_factor(&mut self) -> Result<PolicyAst, String> {
        self.skip_whitespace();
        if self.consume_char('(') {
            let expr = self.parse_or()?;
            self.skip_whitespace();
            if !self.consume_char(')') {
                return Err("unterminated parenthesized policy expression".to_owned());
            }
            return Ok(expr);
        }

        self.parse_term()
    }

    fn parse_term(&mut self) -> Result<PolicyAst, String> {
        let start = self.cursor;
        let mut depth = 0usize;
        while let Some(ch) = self.peek() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }

            if depth == 0 {
                let remaining = &self.input[self.cursor..];
                if remaining.starts_with("&&") || remaining.starts_with("||") {
                    break;
                }
            }

            self.cursor += ch.len_utf8();
        }

        let term = self.input[start..self.cursor].trim();
        if term.is_empty() {
            return Err("policy expression contains an empty term".to_owned());
        }
        Ok(PolicyAst::Term(term.to_owned()))
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        if self.input[self.cursor..].starts_with(expected) {
            self.cursor += expected.len();
            true
        } else {
            false
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        match self.peek() {
            Some(ch) if ch == expected => {
                self.cursor += ch.len_utf8();
                true
            }
            _ => false,
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.cursor += ch.len_utf8();
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.cursor..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.input.len()
    }
}
