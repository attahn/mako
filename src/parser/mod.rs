//! Recursive-descent parser for Mako.

use crate::ast::*;
use crate::lexer::{Token, TokenKind};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("parse error at {line}:{col}: {message}")]
    Message {
        message: String,
        line: usize,
        col: usize,
    },
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while !self.is_eof() {
            if matches!(self.peek_kind(), TokenKind::Import) {
                items.extend(self.parse_import()?);
            } else {
                items.push(self.parse_item()?);
            }
        }
        Ok(Program { items })
    }

    /// Alias used by `mako fmt` tests and tooling.
    #[allow(dead_code)]
    pub fn parse_program(self) -> Result<Program, ParseError> {
        self.parse()
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn bump(&mut self) -> &Token {
        let t = &self.tokens[self.pos];
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(&kind) {
            self.bump();
            Ok(())
        } else {
            Err(self.err(format!(
                "expected {}, found {}",
                crate::diag::friendly_token(&format!("{kind:?}")),
                crate::diag::friendly_token(&format!("{}", self.peek_kind()))
            )))
        }
    }

    fn err(&self, message: String) -> ParseError {
        let t = self.peek();
        ParseError::Message {
            message,
            line: t.line,
            col: t.col,
        }
    }

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        let mut derives = Vec::new();
        while matches!(self.peek_kind(), TokenKind::Hash) {
            derives.extend(self.parse_attr_derives()?);
        }
        match self.peek_kind() {
            TokenKind::Fn => Ok(Item::Fn(self.parse_fn()?)),
            TokenKind::Struct => {
                let mut s = self.parse_struct()?;
                s.derives = derives;
                Ok(Item::Struct(s))
            }
            TokenKind::Enum => Ok(Item::Enum(self.parse_enum()?)),
            TokenKind::Actor => Ok(Item::Actor(self.parse_actor()?)),
            TokenKind::Interface => Ok(Item::Interface(self.parse_interface()?)),
            TokenKind::Extern => Ok(Item::ExternC(self.parse_extern_c()?)),
            TokenKind::Const => Ok(Item::Const(self.parse_const()?)),
            TokenKind::Import => Err(self.err("internal: import handled in parse()".into())),
            _ => Err(self.err(format!("expected item, found {}", self.peek_kind()))),
        }
    }

    fn parse_attr_derives(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(TokenKind::Hash)?;
        self.expect(TokenKind::LBracket)?;
        let name = self.expect_ident()?;
        if name != "derive" {
            return Err(self.err(format!(
                "unknown attribute `{name}` (only derive supported)"
            )));
        }
        self.expect(TokenKind::LParen)?;
        let mut out = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                out.push(self.expect_ident()?);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::RBracket)?;
        Ok(out)
    }

    /// Parse one or more imports. Supports:
    /// - `import "path"` / `import "path" as alias` / `import alias "path"`
    /// - Go grouped: `import ( "a" \n alias "b" \n "c" as c )`
    /// - Brace form: `import { "a"; "b" }`
    fn parse_import(&mut self) -> Result<Vec<Item>, ParseError> {
        self.expect(TokenKind::Import)?;
        match self.peek_kind().clone() {
            TokenKind::LParen => {
                self.bump();
                let mut out = Vec::new();
                while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
                    out.push(self.parse_import_spec()?);
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.bump();
                    }
                }
                self.expect(TokenKind::RParen)?;
                if out.is_empty() {
                    return Err(self.err("import () needs at least one path".into()));
                }
                Ok(out)
            }
            TokenKind::LBrace => {
                self.bump();
                let mut out = Vec::new();
                while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                    out.push(self.parse_import_spec()?);
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.bump();
                    }
                }
                self.expect(TokenKind::RBrace)?;
                if out.is_empty() {
                    return Err(self.err("import {} needs at least one path".into()));
                }
                Ok(out)
            }
            TokenKind::String(_) | TokenKind::Ident(_) => Ok(vec![self.parse_import_spec()?]),
            _ => Err(self.err(
                "import expects \"path\", alias \"path\", import ( … ), or import { … }".into(),
            )),
        }
    }

    fn parse_import_spec(&mut self) -> Result<Item, ParseError> {
        // Go-style: alias "path"
        if matches!(self.peek_kind(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::String(_))
            )
        {
            let alias = self.expect_ident()?;
            let path = match self.peek_kind().clone() {
                TokenKind::String(p) => {
                    self.bump();
                    p
                }
                _ => return Err(self.err("import spec expects alias followed by \"path\"".into())),
            };
            return Ok(Item::Import {
                path,
                alias: Some(alias),
            });
        }
        match self.peek_kind().clone() {
            TokenKind::String(path) => {
                self.bump();
                let alias = if matches!(self.peek_kind(), TokenKind::As) {
                    self.bump();
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                Ok(Item::Import { path, alias })
            }
            _ => Err(self.err("import spec expects \"path\" or alias \"path\"".into())),
        }
    }

    fn parse_const(&mut self) -> Result<ConstDef, ParseError> {
        self.expect(TokenKind::Const)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::Assign)?;
        let value = self.parse_expr()?;
        if matches!(self.peek_kind(), TokenKind::Semicolon) {
            self.bump();
        }
        Ok(ConstDef { name, value })
    }

    fn parse_actor(&mut self) -> Result<ActorDef, ParseError> {
        self.expect(TokenKind::Actor)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut receives = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            self.expect(TokenKind::Receive)?;
            let message = self.expect_ident()?;
            let body = self.parse_block()?;
            receives.push(ReceiveArm { message, body });
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ActorDef { name, receives })
    }

    fn parse_interface(&mut self) -> Result<InterfaceDef, ParseError> {
        self.expect(TokenKind::Interface)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            self.expect(TokenKind::Fn)?;
            let mname = self.expect_ident()?;
            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !matches!(self.peek_kind(), TokenKind::RParen) {
                loop {
                    // allow `name: Type` or bare Type
                    if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        let save = self.pos;
                        let _ = self.expect_ident()?;
                        if matches!(self.peek_kind(), TokenKind::Colon) {
                            self.bump();
                            params.push(self.parse_type()?);
                        } else {
                            self.pos = save;
                            params.push(self.parse_type()?);
                        }
                    } else {
                        params.push(self.parse_type()?);
                    }
                    if matches!(self.peek_kind(), TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::Arrow)?;
            let ret = self.parse_type()?;
            methods.push((mname, params, ret));
            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                self.bump();
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(InterfaceDef { name, methods })
    }

    fn parse_extern_c(&mut self) -> Result<ExternCDef, ParseError> {
        self.expect(TokenKind::Extern)?;
        // extern "C" fn name(...) -> T
        match self.peek_kind() {
            TokenKind::String(s) if s == "C" => {
                self.bump();
            }
            _ => {
                return Err(self.err("expected \"C\" after extern".into()));
            }
        }
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                let mutable = if matches!(self.peek_kind(), TokenKind::Mut) {
                    self.bump();
                    true
                } else {
                    false
                };
                let pname = self.expect_ident()?;
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                params.push(Param {
                    name: pname,
                    ty,
                    mutable,
                });
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        let ret = if matches!(self.peek_kind(), TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        if matches!(self.peek_kind(), TokenKind::Semicolon) {
            self.bump();
        }
        Ok(ExternCDef { name, params, ret })
    }

    fn parse_fn(&mut self) -> Result<FnDef, ParseError> {
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                let mutable = if matches!(self.peek_kind(), TokenKind::Mut) {
                    self.bump();
                    true
                } else {
                    false
                };
                let pname = self.expect_ident()?;
                self.expect(TokenKind::Colon)?;
                let ty = self.parse_type()?;
                params.push(Param {
                    name: pname,
                    ty,
                    mutable,
                });
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        let ret = if matches!(self.peek_kind(), TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(FnDef {
            name,
            params,
            ret,
            body,
        })
    }

    fn parse_struct(&mut self) -> Result<StructDef, ParseError> {
        self.expect(TokenKind::Struct)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            let fname = self.expect_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            fields.push((fname, ty));
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(StructDef {
            name,
            fields,
            derives: Vec::new(),
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDef, ParseError> {
        self.expect(TokenKind::Enum)?;
        let name = self.expect_ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut variants = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            let vname = self.expect_ident()?;
            let mut fields = Vec::new();
            if matches!(self.peek_kind(), TokenKind::LParen) {
                self.bump();
                if !matches!(self.peek_kind(), TokenKind::RParen) {
                    loop {
                        fields.push(self.parse_type()?);
                        if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
            }
            variants.push(EnumVariant {
                name: vname,
                fields,
            });
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(EnumDef { name, variants })
    }

    fn parse_type(&mut self) -> Result<TypeExpr, ParseError> {
        // Go-like `[]T` or existing `[T]`
        if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.bump();
            if matches!(self.peek_kind(), TokenKind::RBracket) {
                self.bump();
                let inner = self.parse_type()?;
                return Ok(TypeExpr::Array(Box::new(inner)));
            }
            let inner = self.parse_type()?;
            self.expect(TokenKind::RBracket)?;
            return Ok(TypeExpr::Array(Box::new(inner)));
        }
        if matches!(self.peek_kind(), TokenKind::Fn) {
            self.bump();
            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !matches!(self.peek_kind(), TokenKind::RParen) {
                loop {
                    params.push(self.parse_type()?);
                    if matches!(self.peek_kind(), TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::Arrow)?;
            let ret = self.parse_type()?;
            return Ok(TypeExpr::Fn(params, Box::new(ret)));
        }
        let name = self.expect_ident()?;
        // Go-like `map[K]V` (key in brackets, value follows)
        if name == "map" && matches!(self.peek_kind(), TokenKind::LBracket) {
            self.bump();
            let key = self.parse_type()?;
            self.expect(TokenKind::RBracket)?;
            let val = self.parse_type()?;
            return Ok(TypeExpr::Map(Box::new(key), Box::new(val)));
        }
        // Dual generics: Result[T,E] and Result<T,E>
        if matches!(self.peek_kind(), TokenKind::LBracket | TokenKind::Lt) {
            let close = if matches!(self.peek_kind(), TokenKind::LBracket) {
                self.bump();
                TokenKind::RBracket
            } else {
                self.bump();
                TokenKind::Gt
            };
            let mut args = Vec::new();
            if std::mem::discriminant(self.peek_kind()) != std::mem::discriminant(&close) {
                loop {
                    args.push(self.parse_type()?);
                    if matches!(self.peek_kind(), TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            self.expect(close)?;
            return Ok(TypeExpr::Generic(name, args));
        }
        Ok(TypeExpr::Named(name))
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        self.expect(TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek_kind() {
            TokenKind::Hold | TokenKind::Share | TokenKind::Let => self.parse_let(),
            TokenKind::Unsafe => {
                self.bump();
                let body = self.parse_block()?;
                Ok(Stmt::Unsafe { body })
            }
            TokenKind::Return => {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::Semicolon | TokenKind::RBrace) {
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.bump();
                    }
                    return Ok(Stmt::Return(None));
                }
                let e = self.parse_expr()?;
                if matches!(self.peek_kind(), TokenKind::Semicolon) {
                    self.bump();
                }
                Ok(Stmt::Return(Some(e)))
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(None),
            TokenKind::For => self.parse_for(None),
            TokenKind::Break => {
                self.bump();
                let label = if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                if matches!(self.peek_kind(), TokenKind::Semicolon) {
                    self.bump();
                }
                Ok(Stmt::Break(label))
            }
            TokenKind::Continue => {
                self.bump();
                let label = if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                if matches!(self.peek_kind(), TokenKind::Semicolon) {
                    self.bump();
                }
                Ok(Stmt::Continue(label))
            }
            TokenKind::Defer => {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::LBrace) {
                    let body = self.parse_block()?;
                    Ok(Stmt::Defer { body })
                } else {
                    // `defer expr` → single-statement block
                    let e = self.parse_expr()?;
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.bump();
                    }
                    Ok(Stmt::Defer {
                        body: Block {
                            stmts: vec![Stmt::Expr(e)],
                        },
                    })
                }
            }
            TokenKind::Crew => {
                self.bump();
                let name = self.expect_ident()?;
                let body = self.parse_block()?;
                Ok(Stmt::Crew { name, body })
            }
            TokenKind::Arena => {
                self.bump();
                let name = self.expect_ident()?;
                let body = self.parse_block()?;
                Ok(Stmt::Arena { name, body })
            }
            TokenKind::Select => self.parse_select(),
            TokenKind::Ident(_) => {
                // `label: while` / `label: for` — labeled loops for break/continue.
                if self.pos + 1 < self.tokens.len()
                    && matches!(self.tokens[self.pos + 1].kind, TokenKind::Colon)
                    && self.pos + 2 < self.tokens.len()
                    && matches!(
                        self.tokens[self.pos + 2].kind,
                        TokenKind::While | TokenKind::For
                    )
                {
                    let label = self.expect_ident()?;
                    self.expect(TokenKind::Colon)?;
                    return match self.peek_kind() {
                        TokenKind::While => self.parse_while(Some(label)),
                        TokenKind::For => self.parse_for(Some(label)),
                        _ => unreachable!(),
                    };
                }
                // Could be assign, index-assign, field-assign, or expression
                let checkpoint = self.pos;
                let name = self.expect_ident()?;
                if matches!(self.peek_kind(), TokenKind::Assign) {
                    self.bump();
                    let value = self.parse_expr()?;
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.bump();
                    }
                    return Ok(Stmt::Assign { name, value });
                }
                // `name.field = value` or `name.a.b = value` (nested field assign)
                if matches!(self.peek_kind(), TokenKind::Dot) {
                    let mut base = Expr::Ident(name);
                    loop {
                        self.bump(); // .
                        let field = self.expect_ident()?;
                        if matches!(self.peek_kind(), TokenKind::Assign) {
                            self.bump();
                            let value = self.parse_expr()?;
                            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                                self.bump();
                            }
                            return Ok(Stmt::FieldAssign { base, field, value });
                        }
                        if matches!(self.peek_kind(), TokenKind::Dot) {
                            base = Expr::Field {
                                base: Box::new(base),
                                field,
                            };
                            continue;
                        }
                        self.pos = checkpoint;
                        break;
                    }
                } else if matches!(self.peek_kind(), TokenKind::LBracket) {
                    // `name[i] = value`
                    self.bump();
                    let index = self.parse_expr()?;
                    self.expect(TokenKind::RBracket)?;
                    if matches!(self.peek_kind(), TokenKind::Assign) {
                        self.bump();
                        let value = self.parse_expr()?;
                        if matches!(self.peek_kind(), TokenKind::Semicolon) {
                            self.bump();
                        }
                        return Ok(Stmt::IndexAssign {
                            base: Expr::Ident(name),
                            index,
                            value,
                        });
                    }
                    self.pos = checkpoint;
                } else {
                    self.pos = checkpoint;
                }
                let e = self.parse_expr()?;
                if matches!(self.peek_kind(), TokenKind::Semicolon) {
                    self.bump();
                }
                Ok(Stmt::Expr(e))
            }
            _ => {
                let e = self.parse_expr()?;
                if matches!(self.peek_kind(), TokenKind::Semicolon) {
                    self.bump();
                }
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_select(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::Select)?;
        // select timeout <expr> { ch => { ... } ... [default|_ => { ... }] }
        self.expect(TokenKind::Timeout)?;
        let timeout_ms = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;
        let mut arms = Vec::new();
        let mut default_arm = None;
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            let is_default = match self.peek_kind() {
                TokenKind::Default => true,
                TokenKind::Ident(s) if s == "_" => true,
                _ => false,
            };
            if is_default {
                self.bump();
                self.expect(TokenKind::FatArrow)?;
                let body = self.parse_block()?;
                if default_arm.is_some() {
                    return Err(self.err("select has multiple default arms".into()));
                }
                default_arm = Some(body);
            } else {
                let ch = self.expect_ident()?;
                self.expect(TokenKind::FatArrow)?;
                let body = self.parse_block()?;
                arms.push((ch, body));
            }
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokenKind::RBrace)?;
        if arms.is_empty() {
            return Err(self.err("select needs at least one channel arm".into()));
        }
        if arms.len() > 16 {
            return Err(self.err("select supports up to 16 channel arms (Partial)".into()));
        }
        Ok(Stmt::Select {
            timeout_ms,
            arms,
            default_arm,
        })
    }

    fn parse_let(&mut self) -> Result<Stmt, ParseError> {
        let ownership = if matches!(self.peek_kind(), TokenKind::Hold) {
            self.bump();
            Ownership::Hold
        } else if matches!(self.peek_kind(), TokenKind::Share) {
            self.bump();
            Ownership::Share
        } else {
            Ownership::None
        };
        self.expect(TokenKind::Let)?;
        let mutable = if matches!(self.peek_kind(), TokenKind::Mut) {
            self.bump();
            true
        } else {
            false
        };
        if ownership == Ownership::Share && mutable {
            return Err(
                self.err("share bindings are immutable (cannot use `share let mut`)".into())
            );
        }
        let name = self.expect_ident()?;
        // Go-like comma-ok: `let v, ok = m[k]`
        if matches!(self.peek_kind(), TokenKind::Comma) {
            if ownership != Ownership::None {
                return Err(self.err("comma-ok let does not support hold/share".into()));
            }
            self.bump();
            let ok = self.expect_ident()?;
            self.expect(TokenKind::Assign)?;
            let init = self.parse_expr()?;
            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                self.bump();
            }
            let Expr::Index { base, index } = init else {
                return Err(
                    self.err("comma-ok requires map index on the right: `let v, ok = m[k]`".into())
                );
            };
            return Ok(Stmt::LetCommaOk {
                value: name,
                ok,
                mutable,
                base: *base,
                index: *index,
            });
        }
        let ty = if matches!(self.peek_kind(), TokenKind::Colon) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(TokenKind::Assign)?;
        let init = self.parse_expr()?;
        if matches!(self.peek_kind(), TokenKind::Semicolon) {
            self.bump();
        }
        Ok(Stmt::Let {
            name,
            mutable,
            ownership,
            ty,
            init,
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        self.expect(TokenKind::If)?;
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if matches!(self.peek_kind(), TokenKind::Else) {
            self.bump();
            if matches!(self.peek_kind(), TokenKind::If) {
                // else if → wrap as block with single if stmt
                let inner = self.parse_if()?;
                Some(Block { stmts: vec![inner] })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_block,
            else_block,
        })
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek_kind(), TokenKind::Or | TokenKind::PipePipe) {
            self.bump();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_cmp()?;
        while matches!(self.peek_kind(), TokenKind::And | TokenKind::AmpAmp) {
            self.bump();
            let right = self.parse_cmp()?;
            left = Expr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitor()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::Ne,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let right = self.parse_bitor()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// Go precedence 4: `+` `-` `|` `^`
    fn parse_bitor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_bitand()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                TokenKind::Pipe => BinOp::BitOr,
                TokenKind::Caret => BinOp::BitXor,
                _ => break,
            };
            self.bump();
            let right = self.parse_bitand()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// Go precedence 5: `*` `/` `%` `<<` `>>` `&` `&^`
    fn parse_bitand(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                TokenKind::Amp => BinOp::BitAnd,
                TokenKind::AmpCaret => BinOp::BitClear,
                _ => break,
            };
            self.bump();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind() {
            TokenKind::Minus => {
                self.bump();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            TokenKind::Bang | TokenKind::Not => {
                self.bump();
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            TokenKind::Caret => {
                self.bump();
                Ok(Expr::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek_kind(), TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                TokenKind::Dot => {
                    self.bump();
                    if matches!(self.peek_kind(), TokenKind::Join) {
                        self.bump();
                        // join as method: x.join or x.join()
                        if matches!(self.peek_kind(), TokenKind::LParen) {
                            self.bump();
                            self.expect(TokenKind::RParen)?;
                        }
                        expr = Expr::Join(Box::new(expr));
                        continue;
                    }
                    if matches!(self.peek_kind(), TokenKind::Kick) {
                        // crew.kick(expr)
                        self.bump();
                        self.expect(TokenKind::LParen)?;
                        let inner = self.parse_expr()?;
                        self.expect(TokenKind::RParen)?;
                        let crew = match &expr {
                            Expr::Ident(n) => n.clone(),
                            _ => return Err(self.err("kick receiver must be a crew name".into())),
                        };
                        expr = Expr::Kick {
                            crew,
                            expr: Box::new(inner),
                        };
                        continue;
                    }
                    let field = self.expect_ident()?;
                    if matches!(self.peek_kind(), TokenKind::LParen) {
                        self.bump();
                        let mut args = Vec::new();
                        if !matches!(self.peek_kind(), TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if matches!(self.peek_kind(), TokenKind::Comma) {
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        expr = Expr::Method {
                            receiver: Box::new(expr),
                            method: field,
                            args,
                        };
                    } else {
                        expr = Expr::Field {
                            base: Box::new(expr),
                            field,
                        };
                    }
                }
                TokenKind::LBracket => {
                    self.bump();
                    // Index `a[i]` or slice `a[low:high]` / `a[low:high:max]` / `a[:]` / `a[i:]` / `a[:j]`
                    if matches!(self.peek_kind(), TokenKind::Colon) {
                        self.bump();
                        let high =
                            if matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Colon) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            };
                        let max = if matches!(self.peek_kind(), TokenKind::Colon) {
                            self.bump();
                            if matches!(self.peek_kind(), TokenKind::RBracket) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            }
                        } else {
                            None
                        };
                        self.expect(TokenKind::RBracket)?;
                        expr = Expr::Slice {
                            base: Box::new(expr),
                            low: None,
                            high,
                            max,
                        };
                    } else {
                        let first = self.parse_expr()?;
                        if matches!(self.peek_kind(), TokenKind::Colon) {
                            self.bump();
                            let high = if matches!(
                                self.peek_kind(),
                                TokenKind::RBracket | TokenKind::Colon
                            ) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            };
                            let max = if matches!(self.peek_kind(), TokenKind::Colon) {
                                self.bump();
                                if matches!(self.peek_kind(), TokenKind::RBracket) {
                                    None
                                } else {
                                    Some(Box::new(self.parse_expr()?))
                                }
                            } else {
                                None
                            };
                            self.expect(TokenKind::RBracket)?;
                            expr = Expr::Slice {
                                base: Box::new(expr),
                                low: Some(Box::new(first)),
                                high,
                                max,
                            };
                        } else {
                            self.expect(TokenKind::RBracket)?;
                            expr = Expr::Index {
                                base: Box::new(expr),
                                index: Box::new(first),
                            };
                        }
                    }
                }
                TokenKind::Question => {
                    self.bump();
                    expr = Expr::Try(Box::new(expr));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Int(n) => {
                self.bump();
                Ok(Expr::Int(n))
            }
            TokenKind::Float(n) => {
                self.bump();
                Ok(Expr::Float(n))
            }
            TokenKind::True => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            TokenKind::String(s) => {
                self.bump();
                Ok(Expr::String(s))
            }
            TokenKind::Ident(name) => {
                self.bump();
                if name == "make" && matches!(self.peek_kind(), TokenKind::LParen) {
                    self.bump();
                    let ty = self.parse_type()?;
                    let (len, cap) = if matches!(self.peek_kind(), TokenKind::Comma) {
                        self.bump();
                        let len = Some(Box::new(self.parse_expr()?));
                        let cap = if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.bump();
                            Some(Box::new(self.parse_expr()?))
                        } else {
                            None
                        };
                        (len, cap)
                    } else {
                        (None, None)
                    };
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::Make { ty, len, cap });
                }
                // Struct literal: `Person { name: "Ada", age: 36 }`
                // Lookahead: only if `{` is followed by `ident :` (not a block body).
                if matches!(self.peek_kind(), TokenKind::LBrace) {
                    let save = self.pos;
                    self.bump(); // {
                    let is_struct_lit = matches!(self.peek_kind(), TokenKind::Ident(_)) && {
                        let after_ident = self.pos + 1;
                        after_ident < self.tokens.len()
                            && matches!(self.tokens[after_ident].kind, TokenKind::Colon)
                    };
                    if is_struct_lit {
                        let mut fields = Vec::new();
                        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                            let fname = self.expect_ident()?;
                            self.expect(TokenKind::Colon)?;
                            let fval = self.parse_expr()?;
                            fields.push((fname, fval));
                            if matches!(self.peek_kind(), TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        self.expect(TokenKind::RBrace)?;
                        return Ok(Expr::StructLit { name, fields });
                    }
                    self.pos = save;
                }
                Ok(Expr::Ident(name))
            }
            TokenKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(e)
            }
            TokenKind::LBracket => {
                self.bump();
                // Go-like `[]T(args)` conversion — not an array literal.
                if matches!(self.peek_kind(), TokenKind::RBracket) {
                    self.bump();
                    let inner = self.parse_type()?;
                    let ty = TypeExpr::Array(Box::new(inner));
                    self.expect(TokenKind::LParen)?;
                    let mut args = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if matches!(self.peek_kind(), TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return Ok(Expr::Convert { ty, args });
                }
                let mut elems = Vec::new();
                if !matches!(self.peek_kind(), TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(Expr::Array(elems))
            }
            TokenKind::LBrace => Ok(Expr::Block(self.parse_block()?)),
            TokenKind::Fn => self.parse_fn_lambda(),
            TokenKind::Pipe => self.parse_lambda(),
            TokenKind::Match => self.parse_match(),
            TokenKind::Fan => {
                self.bump();
                self.expect(TokenKind::LParen)?;
                let collection = self.parse_expr()?;
                self.expect(TokenKind::Comma)?;
                let mapper = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(Expr::Fan {
                    collection: Box::new(collection),
                    mapper: Box::new(mapper),
                })
            }
            _ => Err(self.err(format!("unexpected expression: {}", self.peek_kind()))),
        }
    }

    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::Pipe)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::Pipe) {
            loop {
                params.push(self.expect_ident()?);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::Pipe)?;
        let body = if matches!(self.peek_kind(), TokenKind::LBrace) {
            Expr::Block(self.parse_block()?)
        } else {
            self.parse_expr()?
        };
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn parse_fn_lambda(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::Fn)?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                params.push(self.expect_ident()?);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        let body = if matches!(self.peek_kind(), TokenKind::LBrace) {
            Expr::Block(self.parse_block()?)
        } else {
            self.parse_expr()?
        };
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn parse_match(&mut self) -> Result<Expr, ParseError> {
        self.expect(TokenKind::Match)?;
        let scrutinee = self.parse_expr()?;
        self.expect(TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace) {
            let pattern = self.parse_pattern()?;
            self.expect(TokenKind::FatArrow)?;
            let body = self.parse_expr()?;
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.bump();
            }
            arms.push(MatchArm { pattern, body });
        }
        self.expect(TokenKind::RBrace)?;
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let first = self.parse_pattern_atom()?;
        if !matches!(self.peek_kind(), TokenKind::Pipe) {
            return Ok(first);
        }
        let mut alts = vec![first];
        while matches!(self.peek_kind(), TokenKind::Pipe) {
            self.bump();
            alts.push(self.parse_pattern_atom()?);
        }
        Ok(Pattern::Or(alts))
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Ident(name) if name == "_" => {
                self.bump();
                Ok(Pattern::Wildcard)
            }
            TokenKind::Ident(name) => {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.bump();
                    let mut bindings = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        loop {
                            bindings.push(self.expect_ident()?);
                            if matches!(self.peek_kind(), TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    Ok(Pattern::Variant { name, bindings })
                } else {
                    Ok(Pattern::Ident(name))
                }
            }
            TokenKind::Int(_) | TokenKind::True | TokenKind::False | TokenKind::String(_) => {
                Ok(Pattern::Literal(self.parse_primary()?))
            }
            _ => Err(self.err("expected pattern".into())),
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            // Allow some keywords as identifiers in type/name positions when needed
            other => Err(self.err(format!("expected identifier, found {other}"))),
        }
    }

    /// `for` binders: `i` / `_` / `i, v` / `_, v`
    fn expect_binder(&mut self) -> Result<String, ParseError> {
        self.expect_ident()
    }

    /// Go-like forms:
    ///   `for i, v in range s { ... }`
    ///   `for i in range s { ... }`
    ///   `for _, v in range s { ... }`
    ///   `for range s { ... }`
    /// Legacy:
    ///   `for i in n` / `for v in arr`
    fn parse_while(&mut self, label: Option<String>) -> Result<Stmt, ParseError> {
        self.bump(); // while
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { label, cond, body })
    }

    fn parse_for(&mut self, label: Option<String>) -> Result<Stmt, ParseError> {
        self.bump(); // for
                     // `for range expr { ... }` — no binders
        if matches!(self.peek_kind(), TokenKind::Range) {
            self.bump();
            let iter = self.parse_expr()?;
            let body = self.parse_block()?;
            return Ok(Stmt::For {
                label,
                binders: vec![],
                is_range: true,
                iter,
                body,
            });
        }
        let mut binders = vec![self.expect_binder()?];
        if matches!(self.peek_kind(), TokenKind::Comma) {
            self.bump();
            binders.push(self.expect_binder()?);
        }
        if binders.len() > 2 {
            return Err(self.err("for supports at most two binders (index, value)".into()));
        }
        self.expect(TokenKind::In)?;
        let is_range = if matches!(self.peek_kind(), TokenKind::Range) {
            self.bump();
            true
        } else {
            false
        };
        if binders.len() == 2 && !is_range {
            return Err(self.err("two binders require `range` (e.g. `for i, v in range s`)".into()));
        }
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            label,
            binders,
            is_range,
            iter,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn parse_simple_fn() {
        let src = "fn add(a: int, b: int) -> int { return a + b }";
        let tokens = Lexer::new(src).tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        assert_eq!(prog.items.len(), 1);
    }

    #[test]
    fn parse_grouped_import_paren() {
        let src = r#"
import (
    "strings"
    lib "./x.mko"
    "path" as p
)
fn main() {}
"#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        let imports: Vec<_> = prog
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Import { path, alias } => Some((path.as_str(), alias.as_deref())),
                _ => None,
            })
            .collect();
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0], ("strings", None));
        assert_eq!(imports[1], ("./x.mko", Some("lib")));
        assert_eq!(imports[2], ("path", Some("p")));
    }

    #[test]
    fn parse_grouped_import_brace() {
        let src = r#"import { "strings"; "path" } fn main() {}"#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        let n = prog
            .items
            .iter()
            .filter(|i| matches!(i, Item::Import { .. }))
            .count();
        assert_eq!(n, 2);
    }
}
