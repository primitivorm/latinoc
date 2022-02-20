//! Attributes injected into the crate root from command line using `-Z crate-attr`.

use latinoc_ast::attr::mk_attr;
use latinoc_ast::token;
use latinoc_ast::{self as ast, AttrItem, AttrStyle};
use rustc_session::parse::ParseSess;
use latinoc_span::FileName;

pub fn inject(mut krate: ast::Crate, parse_sess: &ParseSess, attrs: &[String]) -> ast::Crate {
    for raw_attr in attrs {
        let mut parser = latinoc_parse::new_parser_from_source_str(
            parse_sess,
            FileName::cli_crate_attr_source_code(&raw_attr),
            raw_attr.clone(),
        );

        let start_span = parser.token.span;
        let AttrItem { path, args, tokens: _ } = match parser.parse_attr_item(false) {
            Ok(ai) => ai,
            Err(mut err) => {
                err.emit();
                continue;
            }
        };
        let end_span = parser.token.span;
        if parser.token != token::Eof {
            parse_sess.span_diagnostic.span_err(start_span.to(end_span), "invalid crate attribute");
            continue;
        }

        krate.attrs.push(mk_attr(AttrStyle::Inner, path, args, start_span.to(end_span)));
    }

    krate
}