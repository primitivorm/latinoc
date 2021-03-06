use crate::lexer::unicode_chars::UNICODE_ARRAY;
use latinoc_lexer::unescape::{self, Mode};
use latinoc_lexer::{Base, DocStyle, RawStrError};
use rustc_ast::ast::{self, AttrStyle};
use rustc_ast::token::{self, CommentKind, Token, TokenKind};
use rustc_ast::tokenstream::{Spacing, TokenStream};
use rustc_ast::util::unicode::contains_text_flow_control_chars;
use rustc_errors::{error_code, Applicability, DiagnosticBuilder, FatalError, PResult};
use rustc_session::lint::builtin::{
    RUST_2021_PREFIXES_INCOMPATIBLE_SYNTAX, TEXT_DIRECTION_CODEPOINT_IN_COMMENT,
};
use rustc_session::lint::BuiltinLintDiagnostics;
use rustc_session::parse::ParseSess;
use rustc_span::symbol::{sym, Symbol};
use rustc_span::{edition::Edition, BytePos, Pos, Span};

use tracing::debug;

mod tokentrees;
mod unescape_error_reporting;
mod unicode_chars;

use unescape_error_reporting::{emit_unescape_error, escaped_char};

#[derive(Clone, Debug)]
pub struct UnmatchedBrace {
    pub expected_delim: token::DelimToken,
    pub found_delim: Option<token::DelimToken>,
    pub found_span: Span,
    pub unclosed_span: Option<Span>,
    pub candidate_span: Option<Span>,
}

crate fn parse_token_trees<'a>(
    sess: &'a ParseSess,
    src: &'a str,
    start_pos: BytePos,
    override_span: Option<Span>,
) -> (PResult<'a, TokenStream>, Vec<UnmatchedBrace>) {
    StringReader {
        sess,
        start_pos,
        pos: start_pos,
        end_src_index: src.len(),
        src,
        override_span,
    }
    .into_token_trees()
}

struct StringReader<'a> {
    sess: &'a ParseSess,
    /// Initial position, read-only.
    start_pos: BytePos,
    /// The absolute offset within the source_map of the current character.
    pos: BytePos,
    /// Stop reading src at this index.
    end_src_index: usize,
    /// Source text to tokenize.
    src: &'a str,
    override_span: Option<Span>,
}

impl<'a> StringReader<'a> {
    fn mk_sp(&self, lo: BytePos, hi: BytePos) -> Span {
        self.override_span
            .unwrap_or_else(|| Span::with_root_ctxt(lo, hi))
    }

    /// Returns the next token, and info about preceding whitespace, if any.
    fn next_token(&mut self) -> (Spacing, Token) {
        let mut spacing = Spacing::Joint;

        // Skip `#!` at the start of the file
        let start_src_index = self.src_index(self.pos);
        let text: &str = &self.src[start_src_index..self.end_src_index];
        let is_beginning_of_file = self.pos == self.start_pos;
        if is_beginning_of_file {
            if let Some(shebang_len) = latinoc_lexer::strip_shebang(text) {
                self.pos = self.pos + BytePos::from_usize(shebang_len);
                spacing = Spacing::Alone;
            }
        }

        // Skip trivial (whitespace & comments) tokens
        loop {
            let start_src_index = self.src_index(self.pos);
            let text: &str = &self.src[start_src_index..self.end_src_index];

            if text.is_empty() {
                let span = self.mk_sp(self.pos, self.pos);
                return (spacing, Token::new(token::Eof, span));
            }

            let token = latinoc_lexer::first_token(text);

            let start = self.pos;
            self.pos = self.pos + BytePos::from_usize(token.len);

            debug!("next_token: {:?}({:?})", token.kind, self.str_from(start));

            match self.cook_lexer_token(token.kind, start) {
                Some(kind) => {
                    let span = self.mk_sp(start, self.pos);
                    return (spacing, Token::new(kind, span));
                }
                None => spacing = Spacing::Alone,
            }
        }
    }

    /// Report a fatal lexical error with a given span.
    fn fatal_span(&self, sp: Span, m: &str) -> FatalError {
        self.sess.span_diagnostic.span_fatal(sp, m)
    }

    /// Report a lexical error with a given span.
    fn err_span(&self, sp: Span, m: &str) {
        self.sess.span_diagnostic.struct_span_err(sp, m).emit();
    }

    /// Report a fatal error spanning [`from_pos`, `to_pos`).
    fn fatal_span_(&self, from_pos: BytePos, to_pos: BytePos, m: &str) -> FatalError {
        self.fatal_span(self.mk_sp(from_pos, to_pos), m)
    }

    /// Report a lexical error spanning [`from_pos`, `to_pos`).
    fn err_span_(&self, from_pos: BytePos, to_pos: BytePos, m: &str) {
        self.err_span(self.mk_sp(from_pos, to_pos), m)
    }

    fn struct_fatal_span_char(
        &self,
        from_pos: BytePos,
        to_pos: BytePos,
        m: &str,
        c: char,
    ) -> DiagnosticBuilder<'a> {
        self.sess.span_diagnostic.struct_span_fatal(
            self.mk_sp(from_pos, to_pos),
            &format!("{}: {}", m, escaped_char(c)),
        )
    }

    /// Detect usages of Unicode codepoints changing the direction of the text on screen and loudly
    /// complain about it.
    fn lint_unicode_text_flow(&self, start: BytePos) {
        // Opening delimiter of the length 2 is not included into the comment text.
        let content_start = start + BytePos(2);
        let content = self.str_from(content_start);
        if contains_text_flow_control_chars(content) {
            let span = self.mk_sp(start, self.pos);
            self.sess.buffer_lint_with_diagnostic(
                &TEXT_DIRECTION_CODEPOINT_IN_COMMENT,
                span,
                ast::CRATE_NODE_ID,
                "unicode codepoint changing visible direction of text present in comment",
                BuiltinLintDiagnostics::UnicodeTextFlow(span, content.to_string()),
            );
        }
    }

    /// Turns simple `latinoc_lexer::TokenKind` enum into a rich
    /// `rustc_ast::TokenKind`. This turns strings into interned
    /// symbols and runs additional validation.
    fn cook_lexer_token(
        &self,
        token: latinoc_lexer::TokenKind,
        start: BytePos,
    ) -> Option<TokenKind> {
        Some(match token {
            latinoc_lexer::TokenKind::LineComment { doc_style } => {
                // Skip non-doc comments
                let doc_style = if let Some(doc_style) = doc_style {
                    doc_style
                } else {
                    self.lint_unicode_text_flow(start);
                    return None;
                };

                // Opening delimiter of the length 3 is not included into the symbol.
                let content_start = start + BytePos(3);
                let content = self.str_from(content_start);
                self.cook_doc_comment(content_start, content, CommentKind::Line, doc_style)
            }
            latinoc_lexer::TokenKind::BlockComment {
                doc_style,
                terminated,
            } => {
                if !terminated {
                    let msg = match doc_style {
                        Some(_) => "unterminated block doc-comment",
                        None => "unterminated block comment",
                    };
                    let last_bpos = self.pos;
                    self.sess.span_diagnostic.span_fatal_with_code(
                        self.mk_sp(start, last_bpos),
                        msg,
                        error_code!(E0758),
                    );
                }

                // Skip non-doc comments
                let doc_style = if let Some(doc_style) = doc_style {
                    doc_style
                } else {
                    self.lint_unicode_text_flow(start);
                    return None;
                };

                // Opening delimiter of the length 3 and closing delimiter of the length 2
                // are not included into the symbol.
                let content_start = start + BytePos(3);
                let content_end = self.pos - BytePos(if terminated { 2 } else { 0 });
                let content = self.str_from_to(content_start, content_end);
                self.cook_doc_comment(content_start, content, CommentKind::Block, doc_style)
            }
            latinoc_lexer::TokenKind::Whitespace => return None,
            latinoc_lexer::TokenKind::Ident => {
                let mut ident_start = start;
                let sym = nfc_normalize(self.str_from(ident_start));
                let span = self.mk_sp(start, self.pos);
                self.sess.symbol_gallery.insert(sym, span);
                token::Ident(sym, false)
            }
            latinoc_lexer::TokenKind::Literal { kind, suffix_start } => {
                let suffix_start = start + BytePos(suffix_start as u32);
                let (kind, symbol) = self.cook_lexer_literal(start, suffix_start, kind);
                let suffix = if suffix_start < self.pos {
                    let string = self.str_from(suffix_start);
                    if string == "_" {
                        self.sess
                            .span_diagnostic
                            .struct_span_warn(
                                self.mk_sp(suffix_start, self.pos),
                                "underscore literal suffix is not allowed",
                            )
                            .warn(
                                "this was previously accepted by the compiler but is \
                                   being phased out; it will become a hard error in \
                                   a future release!",
                            )
                            .note(
                                "see issue #42326 \
                                 <https://github.com/rust-lang/rust/issues/42326> \
                                 for more information",
                            )
                            .emit();
                        None
                    } else {
                        Some(Symbol::intern(string))
                    }
                } else {
                    None
                };
                token::Literal(token::Lit {
                    kind,
                    symbol,
                    suffix,
                })
            }
            latinoc_lexer::TokenKind::Semi => token::Semi,
            latinoc_lexer::TokenKind::Comma => token::Comma,
            latinoc_lexer::TokenKind::Dot => token::Dot,
            latinoc_lexer::TokenKind::OpenParen => token::OpenDelim(token::Paren),
            latinoc_lexer::TokenKind::CloseParen => token::CloseDelim(token::Paren),
            latinoc_lexer::TokenKind::OpenBrace => token::OpenDelim(token::Brace),
            latinoc_lexer::TokenKind::CloseBrace => token::CloseDelim(token::Brace),
            latinoc_lexer::TokenKind::OpenBracket => token::OpenDelim(token::Bracket),
            latinoc_lexer::TokenKind::CloseBracket => token::CloseDelim(token::Bracket),
            latinoc_lexer::TokenKind::At => token::At,
            latinoc_lexer::TokenKind::Pound => token::Pound,
            latinoc_lexer::TokenKind::Tilde => token::Tilde,
            latinoc_lexer::TokenKind::Question => token::Question,
            latinoc_lexer::TokenKind::Colon => token::Colon,
            latinoc_lexer::TokenKind::Dollar => token::Dollar,
            latinoc_lexer::TokenKind::Eq => token::Eq,
            latinoc_lexer::TokenKind::Bang => token::Not,
            latinoc_lexer::TokenKind::Lt => token::Lt,
            latinoc_lexer::TokenKind::Gt => token::Gt,
            latinoc_lexer::TokenKind::Minus => token::BinOp(token::Minus),
            latinoc_lexer::TokenKind::And => token::BinOp(token::And),
            latinoc_lexer::TokenKind::Or => token::BinOp(token::Or),
            latinoc_lexer::TokenKind::Plus => token::BinOp(token::Plus),
            latinoc_lexer::TokenKind::Star => token::BinOp(token::Star),
            latinoc_lexer::TokenKind::Slash => token::BinOp(token::Slash),
            latinoc_lexer::TokenKind::Caret => token::BinOp(token::Caret),
            latinoc_lexer::TokenKind::Percent => token::BinOp(token::Percent),
            latinoc_lexer::TokenKind::Unknown /*| latinoc_lexer::TokenKind::InvalidIdent*/ => {
                let c = self.str_from(start).chars().next().unwrap();
                let mut err =
                    self.struct_fatal_span_char(start, self.pos, "unknown start of token", c);
                // FIXME: the lexer could be used to turn the ASCII version of unicode homoglyphs,
                // instead of keeping a table in `check_for_substitution`into the token. Ideally,
                // this should be inside `latinoc_lexer`. However, we should first remove compound
                // tokens like `<<` from `latinoc_lexer`, and then add fancier error recovery to it,
                // as there will be less overall work to do this way.
                let token = unicode_chars::check_for_substitution(self, start, c, &mut err);
                if c == '\x00' {
                    err.help("source files must contain UTF-8 encoded text, unexpected null bytes might occur when a different encoding is used");
                }
                err.emit();
                token?
            }
        })
    }

    fn cook_doc_comment(
        &self,
        content_start: BytePos,
        content: &str,
        comment_kind: CommentKind,
        doc_style: DocStyle,
    ) -> TokenKind {
        if content.contains('\r') {
            for (idx, _) in content.char_indices().filter(|&(_, c)| c == '\r') {
                self.err_span_(
                    content_start + BytePos(idx as u32),
                    content_start + BytePos(idx as u32 + 1),
                    match comment_kind {
                        CommentKind::Line => "bare CR not allowed in doc-comment",
                        CommentKind::Block => "bare CR not allowed in block doc-comment",
                    },
                );
            }
        }

        let attr_style = match doc_style {
            DocStyle::Outer => AttrStyle::Outer,
            DocStyle::Inner => AttrStyle::Inner,
        };

        token::DocComment(comment_kind, attr_style, Symbol::intern(content))
    }

    fn cook_lexer_literal(
        &self,
        start: BytePos,
        suffix_start: BytePos,
        kind: latinoc_lexer::LiteralKind,
    ) -> (token::LitKind, Symbol) {
        // prefix means `"` or `br"` or `r###"`, ...
        let (lit_kind, mode, prefix_len, postfix_len) = match kind {
            latinoc_lexer::LiteralKind::Char { terminated } => {
                if !terminated {
                    self.sess.span_diagnostic.span_fatal_with_code(
                        self.mk_sp(start, suffix_start),
                        "unterminated character literal",
                        error_code!(E0762),
                    )
                }
                (token::Char, Mode::Char, 1, 1) // ' '
            }
            latinoc_lexer::LiteralKind::Byte { terminated } => {
                if !terminated {
                    self.sess.span_diagnostic.span_fatal_with_code(
                        self.mk_sp(start + BytePos(1), suffix_start),
                        "unterminated byte constant",
                        error_code!(E0763),
                    )
                }
                (token::Byte, Mode::Byte, 2, 1) // b' '
            }
            latinoc_lexer::LiteralKind::Str { terminated } => {
                if !terminated {
                    self.sess.span_diagnostic.span_fatal_with_code(
                        self.mk_sp(start, suffix_start),
                        "unterminated double quote string",
                        error_code!(E0765),
                    )
                }
                (token::Str, Mode::Str, 1, 1) // " "
            }
            latinoc_lexer::LiteralKind::ByteStr { terminated } => {
                if !terminated {
                    self.sess.span_diagnostic.span_fatal_with_code(
                        self.mk_sp(start + BytePos(1), suffix_start),
                        "unterminated double quote byte string",
                        error_code!(E0766),
                    )
                }
                (token::ByteStr, Mode::ByteStr, 2, 1) // b" "
            }
            latinoc_lexer::LiteralKind::RawStr { n_hashes, err } => {
                self.report_raw_str_error(start, err);
                let n = u32::from(n_hashes);
                (token::StrRaw(n_hashes), Mode::RawStr, 2 + n, 1 + n) // r##" "##
            }
            latinoc_lexer::LiteralKind::RawByteStr { n_hashes, err } => {
                self.report_raw_str_error(start, err);
                let n = u32::from(n_hashes);
                (token::ByteStrRaw(n_hashes), Mode::RawByteStr, 3 + n, 1 + n) // br##" "##
            }
            latinoc_lexer::LiteralKind::Int { base, empty_int } => {
                return if empty_int {
                    self.sess
                        .span_diagnostic
                        .struct_span_err_with_code(
                            self.mk_sp(start, suffix_start),
                            "no valid digits found for number",
                            error_code!(E0768),
                        )
                        .emit();
                    (token::Integer, sym::integer(0))
                } else {
                    self.validate_int_literal(base, start, suffix_start);
                    (token::Integer, self.symbol_from_to(start, suffix_start))
                };
            }
            latinoc_lexer::LiteralKind::Float {
                base,
                empty_exponent,
            } => {
                if empty_exponent {
                    self.err_span_(start, self.pos, "expected at least one digit in exponent");
                }

                match base {
                    Base::Hexadecimal => self.err_span_(
                        start,
                        suffix_start,
                        "hexadecimal float literal is not supported",
                    ),
                    Base::Octal => {
                        self.err_span_(start, suffix_start, "octal float literal is not supported")
                    }
                    Base::Binary => {
                        self.err_span_(start, suffix_start, "binary float literal is not supported")
                    }
                    _ => (),
                }

                let id = self.symbol_from_to(start, suffix_start);
                return (token::Float, id);
            }
        };
        let content_start = start + BytePos(prefix_len);
        let content_end = suffix_start - BytePos(postfix_len);
        let id = self.symbol_from_to(content_start, content_end);
        self.validate_literal_escape(mode, content_start, content_end, prefix_len, postfix_len);
        (lit_kind, id)
    }

    #[inline]
    fn src_index(&self, pos: BytePos) -> usize {
        (pos - self.start_pos).to_usize()
    }

    /// Slice of the source text from `start` up to but excluding `self.pos`,
    /// meaning the slice does not include the character `self.ch`.
    fn str_from(&self, start: BytePos) -> &str {
        self.str_from_to(start, self.pos)
    }

    /// As symbol_from, with an explicit endpoint.
    fn symbol_from_to(&self, start: BytePos, end: BytePos) -> Symbol {
        debug!("taking an ident from {:?} to {:?}", start, end);
        Symbol::intern(self.str_from_to(start, end))
    }

    /// Slice of the source text spanning from `start` up to but excluding `end`.
    fn str_from_to(&self, start: BytePos, end: BytePos) -> &str {
        &self.src[self.src_index(start)..self.src_index(end)]
    }

    fn report_raw_str_error(&self, start: BytePos, opt_err: Option<RawStrError>) {
        match opt_err {
            Some(RawStrError::InvalidStarter { bad_char }) => {
                self.report_non_started_raw_string(start, bad_char)
            }
            Some(RawStrError::NoTerminator {
                expected,
                found,
                possible_terminator_offset,
            }) => self.report_unterminated_raw_string(
                start,
                expected,
                possible_terminator_offset,
                found,
            ),
            Some(RawStrError::TooManyDelimiters { found }) => {
                self.report_too_many_hashes(start, found)
            }
            None => (),
        }
    }

    fn report_non_started_raw_string(&self, start: BytePos, bad_char: char) -> ! {
        self.struct_fatal_span_char(
            start,
            self.pos,
            "found invalid character; only `#` is allowed in raw string delimitation",
            bad_char,
        )
        .emit();
        FatalError.raise()
    }

    fn report_unterminated_raw_string(
        &self,
        start: BytePos,
        n_hashes: usize,
        possible_offset: Option<usize>,
        found_terminators: usize,
    ) -> ! {
        let mut err = self.sess.span_diagnostic.struct_span_fatal_with_code(
            self.mk_sp(start, start),
            "unterminated raw string",
            error_code!(E0748),
        );

        err.span_label(self.mk_sp(start, start), "unterminated raw string");

        if n_hashes > 0 {
            err.note(&format!(
                "this raw string should be terminated with `\"{}`",
                "#".repeat(n_hashes)
            ));
        }

        if let Some(possible_offset) = possible_offset {
            let lo = start + BytePos(possible_offset as u32);
            let hi = lo + BytePos(found_terminators as u32);
            let span = self.mk_sp(lo, hi);
            err.span_suggestion(
                span,
                "consider terminating the string here",
                "#".repeat(n_hashes),
                Applicability::MaybeIncorrect,
            );
        }

        err.emit();
        FatalError.raise()
    }

    /// Note: It was decided to not add a test case, because it would be too big.
    /// <https://github.com/rust-lang/rust/pull/50296#issuecomment-392135180>
    fn report_too_many_hashes(&self, start: BytePos, found: usize) -> ! {
        self.fatal_span_(
            start,
            self.pos,
            &format!(
                "too many `#` symbols: raw strings may be delimited \
                by up to 65535 `#` symbols, but found {}",
                found
            ),
        )
        .raise();
    }

    fn validate_literal_escape(
        &self,
        mode: Mode,
        content_start: BytePos,
        content_end: BytePos,
        prefix_len: u32,
        postfix_len: u32,
    ) {
        let lit_content = self.str_from_to(content_start, content_end);
        unescape::unescape_literal(lit_content, mode, &mut |range, result| {
            // Here we only check for errors. The actual unescaping is done later.
            if let Err(err) = result {
                let span_with_quotes = self.mk_sp(
                    content_start - BytePos(prefix_len),
                    content_end + BytePos(postfix_len),
                );
                let (start, end) = (range.start as u32, range.end as u32);
                let lo = content_start + BytePos(start);
                let hi = lo + BytePos(end - start);
                let span = self.mk_sp(lo, hi);
                emit_unescape_error(
                    &self.sess.span_diagnostic,
                    lit_content,
                    span_with_quotes,
                    span,
                    mode,
                    range,
                    err,
                );
            }
        });
    }

    fn validate_int_literal(&self, base: Base, content_start: BytePos, content_end: BytePos) {
        let base = match base {
            Base::Binary => 2,
            Base::Octal => 8,
            _ => return,
        };
        let s = self.str_from_to(content_start + BytePos(2), content_end);
        for (idx, c) in s.char_indices() {
            let idx = idx as u32;
            if c != '_' && c.to_digit(base).is_none() {
                let lo = content_start + BytePos(2 + idx);
                let hi = content_start + BytePos(2 + idx + c.len_utf8() as u32);
                self.err_span_(
                    lo,
                    hi,
                    &format!("invalid digit for a base {} literal", base),
                );
            }
        }
    }
}

pub fn nfc_normalize(string: &str) -> Symbol {
    use unicode_normalization::{is_nfc_quick, IsNormalized, UnicodeNormalization};
    match is_nfc_quick(string.chars()) {
        IsNormalized::Yes => Symbol::intern(string),
        _ => {
            let normalized_str: String = string.chars().nfc().collect();
            Symbol::intern(&normalized_str)
        }
    }
}
