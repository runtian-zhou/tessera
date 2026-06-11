use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::{lex, Keyword, Symbol as TokSymbol, Token, TokenKind};
use crate::span::{Node, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_program(input: &str) -> Result<Program, ParseError> {
    let tokens = lex(input).map_err(|diagnostics| ParseError { diagnostics })?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        diagnostics: vec![],
    };
    let program = parser.parse_program();
    if parser.diagnostics.is_empty() {
        Ok(program)
    } else {
        Err(ParseError {
            diagnostics: parser.diagnostics,
        })
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

type PResult<T> = Option<T>;

impl Parser {
    fn parse_program(&mut self) -> Program {
        let start = self.peek().span.start;
        let mut items = vec![];
        while !self.at_eof() {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                self.synchronize_item();
            }
        }
        let end = items.last().map(|it| it.span.end).unwrap_or(start);
        Node::new(Span::new(start, end), ProgramKind { items })
    }

    fn parse_item(&mut self) -> PResult<Item> {
        if self.at_keyword(Keyword::Use) {
            let item = self.parse_use_item()?;
            return Some(Node::new(item.span, ItemKind::Use(item)));
        }
        if self.at_keyword(Keyword::Const) {
            let item = self.parse_const_item()?;
            return Some(Node::new(item.span, ItemKind::Const(item)));
        }
        if self.at_keyword(Keyword::Struct) {
            let item = self.parse_struct_item()?;
            return Some(Node::new(item.span, ItemKind::Struct(item)));
        }
        if self.at_keyword(Keyword::Enum) {
            let item = self.parse_enum_item()?;
            return Some(Node::new(item.span, ItemKind::Enum(item)));
        }
        if self.at_keyword(Keyword::Interface) {
            let item = self.parse_interface_item()?;
            return Some(Node::new(item.span, ItemKind::Interface(item)));
        }
        if self.at_keyword(Keyword::Impl) {
            let item = self.parse_impl_item()?;
            return Some(Node::new(item.span, ItemKind::Impl(item)));
        }
        if self.at_keyword(Keyword::Fn) {
            let item = self.parse_fn_item()?;
            return Some(Node::new(item.span, ItemKind::Fn(item)));
        }
        let span = self.peek().span;
        self.error(span, "expected top-level item");
        None
    }

    fn parse_use_item(&mut self) -> PResult<UseItem> {
        let start = self.expect_keyword(Keyword::Use)?.span.start;
        let path = self.parse_path()?;
        let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
        Some(Node::new(Span::new(start, end), UseItemKind { path }))
    }

    fn parse_const_item(&mut self) -> PResult<ConstItem> {
        let start = self.expect_keyword(Keyword::Const)?.span.start;
        let name = self.expect_ident()?;
        self.expect_symbol(TokSymbol::Colon)?;
        let ty = self.parse_int_ty()?;
        self.expect_symbol(TokSymbol::Eq)?;
        let expr = self.parse_const_expr()?;
        let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            ConstItemKind {
                name: name.kind,
                ty,
                expr,
            },
        ))
    }

    fn parse_struct_item(&mut self) -> PResult<StructItem> {
        let start = self.expect_keyword(Keyword::Struct)?.span.start;
        let name = self.expect_ident()?.kind;
        let generics = self.parse_generic_params();
        let where_predicates = self.parse_where_predicates();
        self.expect_symbol(TokSymbol::LBrace)?;
        let mut fields = vec![];
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            fields.push(self.parse_field()?);
            self.eat_symbol(TokSymbol::Comma);
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            StructItemKind {
                name,
                generics,
                where_predicates,
                fields,
            },
        ))
    }

    fn parse_enum_item(&mut self) -> PResult<EnumItem> {
        let start = self.expect_keyword(Keyword::Enum)?.span.start;
        let name = self.expect_ident()?.kind;
        let generics = self.parse_generic_params();
        let repr = if self.eat_symbol(TokSymbol::Colon).is_some() {
            Some(self.parse_int_ty()?)
        } else {
            None
        };
        let where_predicates = self.parse_where_predicates();
        self.expect_symbol(TokSymbol::LBrace)?;
        let mut variants = vec![];
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            variants.push(self.parse_enum_variant()?);
            self.eat_symbol(TokSymbol::Comma);
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            EnumItemKind {
                name,
                generics,
                where_predicates,
                repr,
                variants,
            },
        ))
    }

    fn parse_enum_variant(&mut self) -> PResult<EnumVariant> {
        let name = self.expect_ident()?;
        let start = name.span.start;
        let payload = if self.eat_symbol(TokSymbol::LParen).is_some() {
            let mut tys = vec![];
            if !self.at_symbol(TokSymbol::RParen) {
                loop {
                    tys.push(self.parse_ty()?);
                    if self.eat_symbol(TokSymbol::Comma).is_none() {
                        break;
                    }
                    if self.at_symbol(TokSymbol::RParen) {
                        break;
                    }
                }
            }
            let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
            Node::new(Span::new(start, end), VariantPayloadKind::Tuple(tys))
        } else if self.eat_symbol(TokSymbol::LBrace).is_some() {
            let mut fields = vec![];
            while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
                fields.push(self.parse_field()?);
                self.eat_symbol(TokSymbol::Comma);
            }
            let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
            Node::new(Span::new(start, end), VariantPayloadKind::Struct(fields))
        } else {
            Node::new(name.span, VariantPayloadKind::Unit)
        };
        let discriminant = if self.eat_symbol(TokSymbol::Eq).is_some() {
            Some(self.parse_const_expr()?)
        } else {
            None
        };
        let end = discriminant
            .as_ref()
            .map(|d| d.span.end)
            .unwrap_or(payload.span.end);
        Some(Node::new(
            Span::new(start, end),
            EnumVariantKind {
                name: name.kind,
                payload,
                discriminant,
            },
        ))
    }

    fn parse_interface_item(&mut self) -> PResult<InterfaceItem> {
        let start = self.expect_keyword(Keyword::Interface)?.span.start;
        let name = self.expect_ident()?.kind;
        let generics = self.parse_generic_params();
        let mut super_interfaces = vec![];
        if self.eat_symbol(TokSymbol::Colon).is_some() {
            loop {
                super_interfaces.push(self.parse_interface_ref()?);
                if self.eat_symbol(TokSymbol::Plus).is_none() {
                    break;
                }
            }
        }
        self.expect_symbol(TokSymbol::LBrace)?;
        let mut members = vec![];
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            if self.at_keyword(Keyword::Const) {
                let sig = self.parse_assoc_const_sig()?;
                members.push(Node::new(sig.span, InterfaceMemberKind::AssocConst(sig)));
            } else if self.at_keyword(Keyword::Fn) {
                let sig = self.parse_method_sig(true)?;
                let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
                let span = Span::new(sig.span.start, end);
                members.push(Node::new(span, InterfaceMemberKind::Method(sig)));
            } else {
                self.error(self.peek().span, "expected interface member");
                self.synchronize_member();
            }
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            InterfaceItemKind {
                name,
                generics,
                super_interfaces,
                members,
            },
        ))
    }

    fn parse_assoc_const_sig(&mut self) -> PResult<AssocConstSig> {
        let start = self.expect_keyword(Keyword::Const)?.span.start;
        let name = self.expect_ident()?.kind;
        self.expect_symbol(TokSymbol::Colon)?;
        let ty = self.parse_int_ty()?;
        let default = if self.eat_symbol(TokSymbol::Eq).is_some() {
            Some(self.parse_const_expr()?)
        } else {
            None
        };
        let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            AssocConstSigKind { name, ty, default },
        ))
    }

    fn parse_impl_item(&mut self) -> PResult<ImplItem> {
        let start = self.expect_keyword(Keyword::Impl)?.span.start;
        let generics = self.parse_generic_params();
        let first = self.parse_ty()?;
        let (interface, self_ty) = if self.eat_keyword(Keyword::For).is_some() {
            let interface = match first.kind {
                TyKind::Path { path, args } => {
                    Node::new(first.span, InterfaceRefKind { path, args })
                }
                _ => {
                    self.error(first.span, "expected interface name before `for`");
                    return None;
                }
            };
            let self_ty = self.parse_ty()?;
            (Some(interface), self_ty)
        } else {
            (None, first)
        };
        let where_predicates = self.parse_where_predicates();
        self.expect_symbol(TokSymbol::LBrace)?;
        let mut members = vec![];
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            if self.at_keyword(Keyword::Const) {
                let assoc = self.parse_assoc_const_impl()?;
                members.push(Node::new(assoc.span, ImplMemberKind::AssocConst(assoc)));
            } else if self.at_keyword(Keyword::Fn) {
                let sig = self.parse_method_sig(true)?;
                let body = self.parse_block_expr()?;
                let span = Span::new(sig.span.start, body.span.end);
                members.push(Node::new(
                    span,
                    ImplMemberKind::Method(Node::new(span, MethodDefKind { sig, body })),
                ));
            } else {
                self.error(self.peek().span, "expected impl member");
                self.synchronize_member();
            }
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            ImplItemKind {
                generics,
                where_predicates,
                interface,
                self_ty,
                members,
            },
        ))
    }

    fn parse_assoc_const_impl(&mut self) -> PResult<AssocConstImpl> {
        let start = self.expect_keyword(Keyword::Const)?.span.start;
        let name = self.expect_ident()?.kind;
        self.expect_symbol(TokSymbol::Colon)?;
        let ty = self.parse_int_ty()?;
        self.expect_symbol(TokSymbol::Eq)?;
        let expr = self.parse_const_expr()?;
        let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            AssocConstImplKind { name, ty, expr },
        ))
    }

    fn parse_fn_item(&mut self) -> PResult<FnItem> {
        let start = self.expect_keyword(Keyword::Fn)?.span.start;
        let name = self.expect_ident()?.kind;
        let generics = self.parse_generic_params();
        let params = self.parse_param_list(false)?;
        let ret = if self.eat_symbol(TokSymbol::Arrow).is_some() {
            self.parse_ty()?
        } else {
            Node::new(self.previous_span(), TyKind::Unit)
        };
        let where_predicates = self.parse_where_predicates();
        let body = self.parse_block_expr()?;
        let end = body.span.end;
        Some(Node::new(
            Span::new(start, end),
            FnItemKind {
                name,
                generics,
                where_predicates,
                params,
                ret,
                body,
            },
        ))
    }

    fn parse_method_sig(&mut self, allow_receiver: bool) -> PResult<MethodSig> {
        let start = self.expect_keyword(Keyword::Fn)?.span.start;
        let name = self.expect_ident()?.kind;
        let generics = self.parse_generic_params();
        self.expect_symbol(TokSymbol::LParen)?;
        let mut receiver = None;
        let mut params = vec![];
        if !self.at_symbol(TokSymbol::RParen) {
            loop {
                if allow_receiver && receiver.is_none() && self.at_keyword(Keyword::SelfLower) {
                    let recv_start = self.bump().span.start;
                    if self.eat_symbol(TokSymbol::Colon).is_some() {
                        self.expect_symbol(TokSymbol::Amp)?;
                        let mutability = if self.eat_keyword(Keyword::Mut).is_some() {
                            Mutability::Mutable
                        } else {
                            Mutability::Shared
                        };
                        self.expect_keyword(Keyword::SelfUpper)?;
                        receiver = Some(Node::new(
                            Span::new(recv_start, self.previous_span().end),
                            ReceiverKind::ByRef { mutability },
                        ));
                    } else {
                        receiver = Some(Node::new(
                            Span::new(recv_start, self.previous_span().end),
                            ReceiverKind::ByValue,
                        ));
                    }
                } else {
                    params.push(self.parse_param()?);
                }
                if self.eat_symbol(TokSymbol::Comma).is_none() {
                    break;
                }
                if self.at_symbol(TokSymbol::RParen) {
                    break;
                }
            }
        }
        self.expect_symbol(TokSymbol::RParen)?;
        let ret = if self.eat_symbol(TokSymbol::Arrow).is_some() {
            self.parse_ty()?
        } else {
            Node::new(self.previous_span(), TyKind::Unit)
        };
        let end = ret.span.end;
        Some(Node::new(
            Span::new(start, end),
            MethodSigKind {
                name,
                generics,
                receiver,
                params,
                ret,
            },
        ))
    }

    fn parse_param_list(&mut self, allow_receiver: bool) -> PResult<Vec<Param>> {
        self.expect_symbol(TokSymbol::LParen)?;
        let mut params = vec![];
        if !self.at_symbol(TokSymbol::RParen) {
            loop {
                if allow_receiver && self.at_keyword(Keyword::SelfLower) {
                    self.error(
                        self.peek().span,
                        "receiver is only valid in method signatures",
                    );
                    return None;
                }
                params.push(self.parse_param()?);
                if self.eat_symbol(TokSymbol::Comma).is_none() {
                    break;
                }
                if self.at_symbol(TokSymbol::RParen) {
                    break;
                }
            }
        }
        self.expect_symbol(TokSymbol::RParen)?;
        Some(params)
    }

    fn parse_param(&mut self) -> PResult<Param> {
        let name = self.expect_ident()?;
        let start = name.span.start;
        self.expect_symbol(TokSymbol::Colon)?;
        let ty = self.parse_ty()?;
        Some(Node::new(
            Span::new(start, ty.span.end),
            ParamKind {
                name: name.kind,
                ty,
            },
        ))
    }

    fn parse_field(&mut self) -> PResult<Field> {
        let name = self.expect_ident()?;
        let start = name.span.start;
        self.expect_symbol(TokSymbol::Colon)?;
        let ty = self.parse_ty()?;
        Some(Node::new(
            Span::new(start, ty.span.end),
            FieldKind {
                name: name.kind,
                ty,
            },
        ))
    }

    fn parse_generic_params(&mut self) -> Vec<GenericParam> {
        let mut params = vec![];
        if self.eat_symbol(TokSymbol::Lt).is_none() {
            return params;
        }
        while !self.at_generic_gt() && !self.at_eof() {
            let start = self.peek().span.start;
            if self.eat_keyword(Keyword::Const).is_some() {
                let name = match self.expect_ident() {
                    Some(name) => name,
                    None => break,
                };
                if self.expect_symbol(TokSymbol::Colon).is_none() {
                    break;
                }
                let Some(ty) = self.parse_int_ty() else {
                    break;
                };
                params.push(Node::new(
                    Span::new(start, self.previous_span().end),
                    GenericParamKind::Const {
                        name: name.kind,
                        ty,
                    },
                ));
            } else {
                let name = match self.expect_ident() {
                    Some(name) => name,
                    None => break,
                };
                let mut bounds = vec![];
                if self.eat_symbol(TokSymbol::Colon).is_some() {
                    loop {
                        if let Some(bound) = self.parse_interface_ref() {
                            bounds.push(bound);
                        }
                        if self.eat_symbol(TokSymbol::Plus).is_none() {
                            break;
                        }
                    }
                }
                params.push(Node::new(
                    Span::new(start, self.previous_span().end),
                    GenericParamKind::Type {
                        name: name.kind,
                        bounds,
                    },
                ));
            }
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
            if self.at_generic_gt() {
                break;
            }
        }
        self.expect_generic_gt();
        params
    }

    fn parse_where_predicates(&mut self) -> Vec<WherePredicate> {
        let mut predicates = vec![];
        if self.eat_keyword(Keyword::Where).is_none() {
            return predicates;
        }
        loop {
            let start = self.peek().span.start;
            if self.looks_like_const_eq() {
                let Some(lhs) = self.parse_const_expr() else {
                    break;
                };
                if self.expect_symbol(TokSymbol::EqEq).is_none() {
                    break;
                }
                let Some(rhs) = self.parse_const_expr() else {
                    break;
                };
                predicates.push(Node::new(
                    Span::new(start, rhs.span.end),
                    WherePredicateKind::ConstEq { lhs, rhs },
                ));
            } else {
                let Some(ty) = self.parse_ty() else {
                    break;
                };
                if self.expect_symbol(TokSymbol::Colon).is_none() {
                    break;
                }
                let Some(interface) = self.parse_interface_ref() else {
                    break;
                };
                predicates.push(Node::new(
                    Span::new(start, interface.span.end),
                    WherePredicateKind::Implements { ty, interface },
                ));
            }
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
        }
        predicates
    }

    fn looks_like_const_eq(&self) -> bool {
        let mut idx = self.pos;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                TokenKind::Symbol(TokSymbol::EqEq) => return true,
                TokenKind::Symbol(TokSymbol::Comma | TokSymbol::LBrace) | TokenKind::Eof => {
                    return false
                }
                _ => idx += 1,
            }
        }
        false
    }

    fn parse_interface_ref(&mut self) -> PResult<InterfaceRef> {
        let path = self.parse_path()?;
        let start = path.span.start;
        let args = self.parse_generic_args();
        let end = args.last().map(|arg| arg.span.end).unwrap_or(path.span.end);
        Some(Node::new(
            Span::new(start, end),
            InterfaceRefKind { path, args },
        ))
    }

    fn parse_ty(&mut self) -> PResult<Ty> {
        let start = self.peek().span.start;
        if self.eat_symbol(TokSymbol::LParen).is_some() {
            if self.eat_symbol(TokSymbol::RParen).is_some() {
                return Some(Node::new(
                    Span::new(start, self.previous_span().end),
                    TyKind::Unit,
                ));
            }
            self.error(
                Span::new(start, self.peek().span.end),
                "expected unit type `()`",
            );
            return None;
        }
        if self.eat_symbol(TokSymbol::Bang).is_some() {
            return Some(Node::new(
                Span::new(start, self.previous_span().end),
                TyKind::Never,
            ));
        }
        if self.eat_symbol(TokSymbol::Amp).is_some() {
            let mutability = if self.eat_keyword(Keyword::Mut).is_some() {
                Mutability::Mutable
            } else {
                Mutability::Shared
            };
            let ty = self.parse_ty()?;
            return Some(Node::new(
                Span::new(start, ty.span.end),
                TyKind::Ref {
                    mutability,
                    ty: Box::new(ty),
                },
            ));
        }
        if self.eat_symbol(TokSymbol::LBracket).is_some() {
            let elem = self.parse_ty()?;
            self.expect_symbol(TokSymbol::Semi)?;
            let len = self.parse_const_expr()?;
            let end = self.expect_symbol(TokSymbol::RBracket)?.span.end;
            return Some(Node::new(
                Span::new(start, end),
                TyKind::Array {
                    elem: Box::new(elem),
                    len,
                },
            ));
        }
        if self.at_keyword(Keyword::Fn) {
            self.bump();
            self.expect_symbol(TokSymbol::LParen)?;
            let mut params = vec![];
            if !self.at_symbol(TokSymbol::RParen) {
                loop {
                    params.push(self.parse_ty()?);
                    if self.eat_symbol(TokSymbol::Comma).is_none() {
                        break;
                    }
                }
            }
            self.expect_symbol(TokSymbol::RParen)?;
            self.expect_symbol(TokSymbol::Arrow)?;
            let ret = self.parse_ty()?;
            return Some(Node::new(
                Span::new(start, ret.span.end),
                TyKind::Fn {
                    params,
                    ret: Box::new(ret),
                },
            ));
        }
        if self.eat_keyword(Keyword::SelfUpper).is_some() {
            return Some(Node::new(
                Span::new(start, self.previous_span().end),
                TyKind::SelfTy,
            ));
        }
        if let Some(int_ty) = self.try_parse_int_ty() {
            return Some(Node::new(
                Span::new(start, self.previous_span().end),
                TyKind::Int(int_ty),
            ));
        }
        if self.at_ident() {
            let path = self.parse_path()?;
            if path.kind.segments.len() == 1 && path.kind.segments[0] == "bool" {
                return Some(Node::new(path.span, TyKind::Bool));
            }
            let args = self.parse_generic_args();
            let end = args.last().map(|arg| arg.span.end).unwrap_or(path.span.end);
            return Some(Node::new(
                Span::new(path.span.start, end),
                TyKind::Path { path, args },
            ));
        }
        self.error(self.peek().span, "expected type");
        None
    }

    fn parse_int_ty(&mut self) -> PResult<IntTy> {
        if let Some(ty) = self.try_parse_int_ty() {
            Some(ty)
        } else {
            self.error(self.peek().span, "expected integer type");
            None
        }
    }

    fn try_parse_int_ty(&mut self) -> Option<IntTy> {
        let text = match &self.peek().kind {
            TokenKind::Ident(text) => text.clone(),
            _ => return None,
        };
        let ty = match text.as_str() {
            "i8" => IntTy::new(Signedness::Signed, IntWidth::W8),
            "i16" => IntTy::new(Signedness::Signed, IntWidth::W16),
            "i32" => IntTy::i32(),
            "i64" => IntTy::i64(),
            "i128" => IntTy::new(Signedness::Signed, IntWidth::W128),
            "isize" => IntTy::new(Signedness::Signed, IntWidth::Size),
            "u8" => IntTy::u8(),
            "u16" => IntTy::new(Signedness::Unsigned, IntWidth::W16),
            "u32" => IntTy::new(Signedness::Unsigned, IntWidth::W32),
            "u64" => IntTy::new(Signedness::Unsigned, IntWidth::W64),
            "u128" => IntTy::new(Signedness::Unsigned, IntWidth::W128),
            "usize" => IntTy::usize(),
            _ => return None,
        };
        self.bump();
        Some(ty)
    }

    fn parse_generic_args(&mut self) -> Vec<GenericArg> {
        let mut args = vec![];
        if self.eat_symbol(TokSymbol::Lt).is_none() {
            return args;
        }
        while !self.at_generic_gt() && !self.at_eof() {
            let start = self.peek().span.start;
            let arg = if self.looks_like_const_generic_arg() {
                let Some(expr) = self.parse_const_expr() else {
                    break;
                };
                Node::new(Span::new(start, expr.span.end), GenericArgKind::Const(expr))
            } else {
                let Some(ty) = self.parse_ty() else {
                    break;
                };
                Node::new(Span::new(start, ty.span.end), GenericArgKind::Ty(ty))
            };
            args.push(arg);
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
            if self.at_generic_gt() {
                break;
            }
        }
        self.expect_generic_gt();
        args
    }

    fn looks_like_const_generic_arg(&self) -> bool {
        if self.at_int()
            || self.at_symbol(TokSymbol::Minus)
            || self.at_symbol(TokSymbol::Plus)
            || self.at_symbol(TokSymbol::Lt)
        {
            return true;
        }

        let mut depth = 0usize;
        let mut idx = self.pos;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                TokenKind::Symbol(TokSymbol::Lt | TokSymbol::LParen | TokSymbol::LBracket) => {
                    depth += 1;
                }
                TokenKind::Symbol(TokSymbol::Gt | TokSymbol::RParen | TokSymbol::RBracket) => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                TokenKind::Symbol(TokSymbol::Shr) if depth == 0 => {
                    return self.token_starts_const_expr(idx + 1);
                }
                TokenKind::Symbol(TokSymbol::Shr) => {
                    if depth <= 1 {
                        return false;
                    }
                    depth -= 2;
                }
                TokenKind::Symbol(TokSymbol::Comma) if depth == 0 => return false,
                TokenKind::Symbol(
                    TokSymbol::Plus
                    | TokSymbol::Minus
                    | TokSymbol::Star
                    | TokSymbol::Slash
                    | TokSymbol::Percent
                    | TokSymbol::Shl
                    | TokSymbol::Amp
                    | TokSymbol::Pipe
                    | TokSymbol::Caret,
                ) if depth == 0 => return true,
                TokenKind::Keyword(Keyword::As) if depth == 0 => return true,
                TokenKind::Eof => return false,
                _ => {}
            }
            idx += 1;
        }
        false
    }

    fn token_starts_const_expr(&self, idx: usize) -> bool {
        matches!(
            self.tokens.get(idx).map(|token| &token.kind),
            Some(TokenKind::Int(_))
                | Some(TokenKind::Ident(_))
                | Some(TokenKind::Symbol(
                    TokSymbol::Plus | TokSymbol::Minus | TokSymbol::LParen | TokSymbol::Lt
                ))
        )
    }

    fn at_generic_gt(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Symbol(TokSymbol::Gt | TokSymbol::Shr)
        )
    }

    fn expect_generic_gt(&mut self) -> PResult<Token> {
        if self.at_symbol(TokSymbol::Gt) {
            return Some(self.bump());
        }
        let token = self.peek().clone();
        if matches!(token.kind, TokenKind::Symbol(TokSymbol::Shr)) {
            self.tokens[self.pos] = Node::new(
                Span::new(token.span.start + 1, token.span.end),
                TokenKind::Symbol(TokSymbol::Gt),
            );
            return Some(Node::new(
                Span::new(token.span.start, token.span.start + 1),
                TokenKind::Symbol(TokSymbol::Gt),
            ));
        }
        self.error(self.peek().span, "expected symbol `Gt`");
        None
    }

    fn parse_const_expr(&mut self) -> PResult<ConstExpr> {
        self.parse_const_binary(0)
    }

    fn parse_const_binary(&mut self, min_prec: u8) -> PResult<ConstExpr> {
        let mut lhs = self.parse_const_unary()?;
        while let Some((op, prec)) = self.peek_const_binary_op() {
            if prec < min_prec {
                break;
            }
            self.bump();
            let rhs = self.parse_const_binary(prec + 1)?;
            let span = lhs.span.join(rhs.span);
            lhs = Node::new(
                span,
                ConstExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            );
        }
        if self.eat_keyword(Keyword::As).is_some() {
            let ty = self.parse_int_ty()?;
            let span = Span::new(lhs.span.start, self.previous_span().end);
            lhs = Node::new(
                span,
                ConstExprKind::Cast {
                    expr: Box::new(lhs),
                    ty,
                },
            );
        }
        Some(lhs)
    }

    fn parse_const_unary(&mut self) -> PResult<ConstExpr> {
        if self.at_symbol(TokSymbol::Plus) {
            let start = self.bump().span.start;
            let expr = self.parse_const_unary()?;
            return Some(Node::new(
                Span::new(start, expr.span.end),
                ConstExprKind::Unary {
                    op: ConstUnaryOp::Plus,
                    expr: Box::new(expr),
                },
            ));
        }
        if self.at_symbol(TokSymbol::Minus) {
            let start = self.bump().span.start;
            let expr = self.parse_const_unary()?;
            return Some(Node::new(
                Span::new(start, expr.span.end),
                ConstExprKind::Unary {
                    op: ConstUnaryOp::Neg,
                    expr: Box::new(expr),
                },
            ));
        }
        self.parse_const_primary()
    }

    fn parse_const_primary(&mut self) -> PResult<ConstExpr> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Some(Node::new(token.span, ConstExprKind::IntLit(value)))
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path()?;
                Some(Node::new(path.span, ConstExprKind::Path(path)))
            }
            TokenKind::Symbol(TokSymbol::LParen) => {
                self.bump();
                let expr = self.parse_const_expr()?;
                let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                Some(Node::new(Span::new(token.span.start, end), expr.kind))
            }
            TokenKind::Symbol(TokSymbol::Lt) => self.parse_assoc_const_expr(),
            _ => {
                self.error(token.span, "expected const expression");
                None
            }
        }
    }

    fn parse_assoc_const_expr(&mut self) -> PResult<ConstExpr> {
        let start = self.expect_symbol(TokSymbol::Lt)?.span.start;
        let ty = self.parse_ty()?;
        self.expect_keyword(Keyword::As)?;
        let interface = self.parse_path()?;
        self.expect_symbol(TokSymbol::Gt)?;
        self.expect_symbol(TokSymbol::ColonColon)?;
        let name = self.expect_ident()?;
        Some(Node::new(
            Span::new(start, name.span.end),
            ConstExprKind::AssocConst {
                ty: Box::new(ty),
                interface,
                name: name.kind,
            },
        ))
    }

    fn peek_const_binary_op(&self) -> Option<(ConstBinaryOp, u8)> {
        let op = match self.peek().kind {
            TokenKind::Symbol(TokSymbol::Star) => (ConstBinaryOp::Mul, 7),
            TokenKind::Symbol(TokSymbol::Slash) => (ConstBinaryOp::Div, 7),
            TokenKind::Symbol(TokSymbol::Percent) => (ConstBinaryOp::Rem, 7),
            TokenKind::Symbol(TokSymbol::Plus) => (ConstBinaryOp::Add, 6),
            TokenKind::Symbol(TokSymbol::Minus) => (ConstBinaryOp::Sub, 6),
            TokenKind::Symbol(TokSymbol::Shl) => (ConstBinaryOp::Shl, 5),
            TokenKind::Symbol(TokSymbol::Shr) => (ConstBinaryOp::Shr, 5),
            TokenKind::Symbol(TokSymbol::Amp) => (ConstBinaryOp::BitAnd, 4),
            TokenKind::Symbol(TokSymbol::Caret) => (ConstBinaryOp::BitXor, 3),
            TokenKind::Symbol(TokSymbol::Pipe) => (ConstBinaryOp::BitOr, 2),
            _ => return None,
        };
        Some(op)
    }

    fn parse_block_expr(&mut self) -> PResult<Expr> {
        let block = self.parse_block()?;
        Some(Node::new(block.span, ExprKind::Block(block)))
    }

    fn parse_block(&mut self) -> PResult<Block> {
        let start = self.expect_symbol(TokSymbol::LBrace)?.span.start;
        let mut stmts = vec![];
        let mut tail = None;
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            if self.at_keyword(Keyword::Let) {
                let let_stmt = self.parse_let_stmt()?;
                let end = self.expect_symbol(TokSymbol::Semi)?.span.end;
                stmts.push(Node::new(
                    Span::new(let_stmt.span.start, end),
                    StmtKind::Let(let_stmt),
                ));
                continue;
            }
            let expr = self.parse_expr()?;
            if self.eat_symbol(TokSymbol::Semi).is_some() {
                let span = Span::new(expr.span.start, self.previous_span().end);
                stmts.push(Node::new(span, StmtKind::Semi(expr)));
            } else if self.at_symbol(TokSymbol::RBrace) {
                tail = Some(Box::new(expr));
                break;
            } else {
                stmts.push(Node::new(expr.span, StmtKind::Expr(expr)));
            }
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(Span::new(start, end), BlockKind { stmts, tail }))
    }

    fn parse_let_stmt(&mut self) -> PResult<LetStmt> {
        let start = self.expect_keyword(Keyword::Let)?.span.start;
        let name = self.expect_ident()?.kind;
        let ty = if self.eat_symbol(TokSymbol::Colon).is_some() {
            Some(self.parse_ty()?)
        } else {
            None
        };
        let init = if self.eat_symbol(TokSymbol::Eq).is_some() {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let end = init
            .as_ref()
            .map(|expr| expr.span.end)
            .or_else(|| ty.as_ref().map(|ty| ty.span.end))
            .unwrap_or(start);
        Some(Node::new(
            Span::new(start, end),
            LetStmtKind { name, ty, init },
        ))
    }

    fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> PResult<Expr> {
        let lhs = self.parse_binary_expr(0)?;
        if self.eat_symbol(TokSymbol::Eq).is_some() {
            let rhs = self.parse_assign()?;
            let span = lhs.span.join(rhs.span);
            Some(Node::new(
                span,
                ExprKind::Assign {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            ))
        } else {
            Some(lhs)
        }
    }

    fn parse_binary_expr(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.parse_unary_expr()?;
        while let Some((op, prec)) = self.peek_binary_op() {
            if prec < min_prec {
                break;
            }
            self.bump();
            let rhs = self.parse_binary_expr(prec + 1)?;
            let span = lhs.span.join(rhs.span);
            lhs = Node::new(
                span,
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            );
        }
        Some(lhs)
    }

    fn parse_unary_expr(&mut self) -> PResult<Expr> {
        if self.at_symbol(TokSymbol::Minus) {
            let start = self.bump().span.start;
            let expr = self.parse_unary_expr()?;
            return Some(Node::new(
                Span::new(start, expr.span.end),
                ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
            ));
        }
        if self.at_symbol(TokSymbol::Bang) {
            let start = self.bump().span.start;
            let expr = self.parse_unary_expr()?;
            return Some(Node::new(
                Span::new(start, expr.span.end),
                ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
            ));
        }
        self.parse_postfix_expr()
    }

    fn parse_postfix_expr(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_primary_expr()?;
        loop {
            if self.eat_symbol(TokSymbol::LParen).is_some() {
                let args = self.parse_expr_list(TokSymbol::RParen)?;
                let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                let span = Span::new(expr.span.start, end);
                expr = Node::new(
                    span,
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                );
                continue;
            }
            if self.eat_symbol(TokSymbol::Dot).is_some() {
                let method_or_field = self.expect_ident()?;
                if self.eat_symbol(TokSymbol::LParen).is_some() {
                    let args = self.parse_expr_list(TokSymbol::RParen)?;
                    let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                    let span = Span::new(expr.span.start, end);
                    expr = Node::new(
                        span,
                        ExprKind::MethodCall {
                            receiver: Box::new(expr),
                            method: method_or_field.kind,
                            args,
                        },
                    );
                } else {
                    let span = Span::new(expr.span.start, method_or_field.span.end);
                    expr = Node::new(
                        span,
                        ExprKind::Field {
                            base: Box::new(expr),
                            name: method_or_field.kind,
                        },
                    );
                }
                continue;
            }
            if self.eat_symbol(TokSymbol::LBracket).is_some() {
                let index = self.parse_expr()?;
                let end = self.expect_symbol(TokSymbol::RBracket)?.span.end;
                let span = Span::new(expr.span.start, end);
                expr = Node::new(
                    span,
                    ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                );
                continue;
            }
            break;
        }
        Some(expr)
    }

    fn parse_primary_expr(&mut self) -> PResult<Expr> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                Some(Node::new(token.span, ExprKind::IntLit(value)))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Some(Node::new(token.span, ExprKind::BoolLit(true)))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Some(Node::new(token.span, ExprKind::BoolLit(false)))
            }
            TokenKind::Keyword(Keyword::Todo) => {
                self.bump();
                Some(Node::new(token.span, ExprKind::Todo))
            }
            TokenKind::Keyword(Keyword::SelfLower) => {
                self.bump();
                Some(Node::new(token.span, ExprKind::Var("self".to_owned())))
            }
            TokenKind::Keyword(Keyword::Return) => {
                self.bump();
                let expr = if self.at_symbol(TokSymbol::Semi) || self.at_symbol(TokSymbol::RBrace) {
                    None
                } else {
                    Some(Box::new(self.parse_expr()?))
                };
                let end = expr.as_ref().map(|e| e.span.end).unwrap_or(token.span.end);
                Some(Node::new(
                    Span::new(token.span.start, end),
                    ExprKind::Return(expr),
                ))
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if_expr(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expr(),
            TokenKind::Symbol(TokSymbol::LBrace) => self.parse_block_expr(),
            TokenKind::Symbol(TokSymbol::LParen) => {
                let start = self.bump().span.start;
                if self.eat_symbol(TokSymbol::RParen).is_some() {
                    Some(Node::new(
                        Span::new(start, self.previous_span().end),
                        ExprKind::UnitLit,
                    ))
                } else {
                    let expr = self.parse_expr()?;
                    let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                    Some(Node::new(Span::new(start, end), expr.kind))
                }
            }
            TokenKind::Ident(_) => self.parse_path_expr(),
            _ => {
                self.error(token.span, "expected expression");
                None
            }
        }
    }

    fn parse_path_expr(&mut self) -> PResult<Expr> {
        let path = self.parse_path()?;
        if self.at_symbol(TokSymbol::LBrace) && self.looks_like_record_literal_body() {
            self.bump();
            let fields = self.parse_field_expr_list(TokSymbol::RBrace)?;
            let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
            if path.kind.segments.len() >= 2 {
                let (enum_path, variant) = split_variant_path(&path);
                return Some(Node::new(
                    Span::new(path.span.start, end),
                    ExprKind::EnumCtor {
                        enum_path,
                        variant,
                        args: Node::new(
                            Span::new(path.span.end, end),
                            EnumCtorArgsKind::Struct(fields),
                        ),
                    },
                ));
            }
            return Some(Node::new(
                Span::new(path.span.start, end),
                ExprKind::StructLit { path, fields },
            ));
        }
        if path.kind.segments.len() >= 2 {
            let (enum_path, variant) = split_variant_path(&path);
            if self.eat_symbol(TokSymbol::LParen).is_some() {
                let args = self.parse_expr_list(TokSymbol::RParen)?;
                let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                return Some(Node::new(
                    Span::new(path.span.start, end),
                    ExprKind::EnumCtor {
                        enum_path,
                        variant,
                        args: Node::new(
                            Span::new(path.span.end, end),
                            EnumCtorArgsKind::Tuple(args),
                        ),
                    },
                ));
            }
            return Some(Node::new(
                path.span,
                ExprKind::EnumCtor {
                    enum_path,
                    variant,
                    args: Node::new(path.span, EnumCtorArgsKind::Unit),
                },
            ));
        }
        Some(Node::new(
            path.span,
            ExprKind::Var(path.kind.segments[0].clone()),
        ))
    }

    fn parse_if_expr(&mut self) -> PResult<Expr> {
        let start = self.expect_keyword(Keyword::If)?.span.start;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        let else_branch = if self.eat_keyword(Keyword::Else).is_some() {
            Some(self.parse_block()?)
        } else {
            None
        };
        let end = else_branch
            .as_ref()
            .map(|b| b.span.end)
            .unwrap_or(then_branch.span.end);
        Some(Node::new(
            Span::new(start, end),
            ExprKind::If {
                cond: Box::new(cond),
                then_branch,
                else_branch,
            },
        ))
    }

    fn parse_match_expr(&mut self) -> PResult<Expr> {
        let start = self.expect_keyword(Keyword::Match)?.span.start;
        let scrutinee = self.parse_expr()?;
        self.expect_symbol(TokSymbol::LBrace)?;
        let mut arms = vec![];
        while !self.at_symbol(TokSymbol::RBrace) && !self.at_eof() {
            let pat = self.parse_pat()?;
            self.expect_symbol(TokSymbol::FatArrow)?;
            let body = self.parse_expr()?;
            let span = Span::new(pat.span.start, body.span.end);
            arms.push(Node::new(span, MatchArmKind { pat, body }));
            self.eat_symbol(TokSymbol::Comma);
        }
        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
        Some(Node::new(
            Span::new(start, end),
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
        ))
    }

    fn parse_pat(&mut self) -> PResult<Pat> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(ref text) if text == "_" => {
                self.bump();
                Some(Node::new(token.span, PatKind::Wildcard))
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path()?;
                if path.kind.segments.len() >= 2 {
                    let (enum_path, variant) = split_variant_path(&path);
                    if self.eat_symbol(TokSymbol::LParen).is_some() {
                        let mut pats = vec![];
                        if !self.at_symbol(TokSymbol::RParen) {
                            loop {
                                pats.push(self.parse_pat()?);
                                if self.eat_symbol(TokSymbol::Comma).is_none() {
                                    break;
                                }
                            }
                        }
                        let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                        return Some(Node::new(
                            Span::new(path.span.start, end),
                            PatKind::EnumVariant {
                                enum_path,
                                variant,
                                args: Node::new(
                                    Span::new(path.span.end, end),
                                    EnumPatArgsKind::Tuple(pats),
                                ),
                            },
                        ));
                    }
                    if self.eat_symbol(TokSymbol::LBrace).is_some() {
                        let fields = self.parse_field_pat_list(TokSymbol::RBrace)?;
                        let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
                        return Some(Node::new(
                            Span::new(path.span.start, end),
                            PatKind::EnumVariant {
                                enum_path,
                                variant,
                                args: Node::new(
                                    Span::new(path.span.end, end),
                                    EnumPatArgsKind::Struct(fields),
                                ),
                            },
                        ));
                    }
                    return Some(Node::new(
                        path.span,
                        PatKind::EnumVariant {
                            enum_path,
                            variant,
                            args: Node::new(path.span, EnumPatArgsKind::Unit),
                        },
                    ));
                }
                if self.eat_symbol(TokSymbol::LBrace).is_some() {
                    let fields = self.parse_field_pat_list(TokSymbol::RBrace)?;
                    let end = self.expect_symbol(TokSymbol::RBrace)?.span.end;
                    return Some(Node::new(
                        Span::new(path.span.start, end),
                        PatKind::Struct { path, fields },
                    ));
                }
                Some(Node::new(
                    path.span,
                    PatKind::Binding {
                        name: path.kind.segments[0].clone(),
                    },
                ))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Some(Node::new(token.span, PatKind::BoolLit(true)))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Some(Node::new(token.span, PatKind::BoolLit(false)))
            }
            TokenKind::Int(value) => {
                self.bump();
                Some(Node::new(token.span, PatKind::IntLit(value)))
            }
            TokenKind::Symbol(TokSymbol::LParen) => {
                let start = self.bump().span.start;
                let end = self.expect_symbol(TokSymbol::RParen)?.span.end;
                Some(Node::new(Span::new(start, end), PatKind::Unit))
            }
            _ => {
                self.error(token.span, "expected pattern");
                None
            }
        }
    }

    fn parse_expr_list(&mut self, end_symbol: TokSymbol) -> PResult<Vec<Expr>> {
        let mut args = vec![];
        if self.at_symbol(end_symbol) {
            return Some(args);
        }
        loop {
            args.push(self.parse_expr()?);
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
            if self.at_symbol(end_symbol) {
                break;
            }
        }
        Some(args)
    }

    fn parse_field_expr_list(&mut self, end_symbol: TokSymbol) -> PResult<Vec<FieldExpr>> {
        let mut fields = vec![];
        if self.at_symbol(end_symbol) {
            return Some(fields);
        }
        loop {
            let name = self.expect_ident()?;
            let start = name.span.start;
            self.expect_symbol(TokSymbol::Colon)?;
            let expr = self.parse_expr()?;
            fields.push(Node::new(
                Span::new(start, expr.span.end),
                FieldExprKind {
                    name: name.kind,
                    expr,
                },
            ));
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
            if self.at_symbol(end_symbol) {
                break;
            }
        }
        Some(fields)
    }

    fn parse_field_pat_list(&mut self, end_symbol: TokSymbol) -> PResult<Vec<FieldPat>> {
        let mut fields = vec![];
        if self.at_symbol(end_symbol) {
            return Some(fields);
        }
        loop {
            let name = self.expect_ident()?;
            let start = name.span.start;
            let pat = if self.eat_symbol(TokSymbol::Colon).is_some() {
                self.parse_pat()?
            } else {
                Node::new(
                    name.span,
                    PatKind::Binding {
                        name: name.kind.clone(),
                    },
                )
            };
            fields.push(Node::new(
                Span::new(start, pat.span.end),
                FieldPatKind {
                    name: name.kind,
                    pat,
                },
            ));
            if self.eat_symbol(TokSymbol::Comma).is_none() {
                break;
            }
            if self.at_symbol(end_symbol) {
                break;
            }
        }
        Some(fields)
    }

    fn peek_binary_op(&self) -> Option<(BinaryOp, u8)> {
        let op = match self.peek().kind {
            TokenKind::Symbol(TokSymbol::PipePipe) => (BinaryOp::Or, 1),
            TokenKind::Symbol(TokSymbol::AmpAmp) => (BinaryOp::And, 2),
            TokenKind::Symbol(TokSymbol::EqEq) => (BinaryOp::Eq, 3),
            TokenKind::Symbol(TokSymbol::Ne) => (BinaryOp::Ne, 3),
            TokenKind::Symbol(TokSymbol::Lt) => (BinaryOp::Lt, 3),
            TokenKind::Symbol(TokSymbol::Le) => (BinaryOp::Le, 3),
            TokenKind::Symbol(TokSymbol::Gt) => (BinaryOp::Gt, 3),
            TokenKind::Symbol(TokSymbol::Ge) => (BinaryOp::Ge, 3),
            TokenKind::Symbol(TokSymbol::Pipe) => (BinaryOp::BitOr, 4),
            TokenKind::Symbol(TokSymbol::Caret) => (BinaryOp::BitXor, 5),
            TokenKind::Symbol(TokSymbol::Amp) => (BinaryOp::BitAnd, 6),
            TokenKind::Symbol(TokSymbol::Shl) => (BinaryOp::Shl, 7),
            TokenKind::Symbol(TokSymbol::Shr) => (BinaryOp::Shr, 7),
            TokenKind::Symbol(TokSymbol::Plus) => (BinaryOp::Add, 8),
            TokenKind::Symbol(TokSymbol::Minus) => (BinaryOp::Sub, 8),
            TokenKind::Symbol(TokSymbol::Star) => (BinaryOp::Mul, 9),
            TokenKind::Symbol(TokSymbol::Slash) => (BinaryOp::Div, 9),
            TokenKind::Symbol(TokSymbol::Percent) => (BinaryOp::Rem, 9),
            _ => return None,
        };
        Some(op)
    }

    fn parse_path(&mut self) -> PResult<Path> {
        let first = self.expect_ident()?;
        let start = first.span.start;
        let mut segments = vec![first.kind];
        let mut end = first.span.end;
        while self.eat_symbol(TokSymbol::ColonColon).is_some() {
            let seg = self.expect_ident()?;
            end = seg.span.end;
            segments.push(seg.kind);
        }
        Some(Node::new(Span::new(start, end), PathKind { segments }))
    }

    fn expect_ident(&mut self) -> PResult<Node<String>> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::Ident(text) => {
                self.bump();
                Some(Node::new(token.span, text))
            }
            _ => {
                self.error(token.span, "expected identifier");
                None
            }
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> PResult<Token> {
        if self.at_keyword(keyword) {
            Some(self.bump())
        } else {
            self.error(self.peek().span, format!("expected keyword `{keyword:?}`"));
            None
        }
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> Option<Token> {
        if self.at_keyword(keyword) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn expect_symbol(&mut self, symbol: TokSymbol) -> PResult<Token> {
        if self.at_symbol(symbol) {
            Some(self.bump())
        } else {
            self.error(self.peek().span, format!("expected symbol `{symbol:?}`"));
            None
        }
    }

    fn eat_symbol(&mut self, symbol: TokSymbol) -> Option<Token> {
        if self.at_symbol(symbol) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn at_ident(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Ident(_))
    }

    fn at_int(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Int(_))
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek().kind, TokenKind::Keyword(k) if k == keyword)
    }

    fn at_symbol(&self, symbol: TokSymbol) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(s) if s == symbol)
    }

    fn looks_like_record_literal_body(&self) -> bool {
        let lbrace_pos = self.pos;
        matches!(
            (
                self.tokens.get(lbrace_pos + 1).map(|token| &token.kind),
                self.tokens.get(lbrace_pos + 2).map(|token| &token.kind),
            ),
            (Some(TokenKind::Symbol(TokSymbol::RBrace)), _)
                | (
                    Some(TokenKind::Ident(_)),
                    Some(TokenKind::Symbol(TokSymbol::Colon))
                )
        )
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        token
    }

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map(|tok| tok.span)
            .unwrap_or_default()
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(span, message));
    }

    fn synchronize_item(&mut self) {
        while !self.at_eof() {
            if matches!(
                self.peek().kind,
                TokenKind::Keyword(
                    Keyword::Const
                        | Keyword::Struct
                        | Keyword::Enum
                        | Keyword::Interface
                        | Keyword::Impl
                        | Keyword::Fn
                )
            ) {
                return;
            }
            self.bump();
        }
    }

    fn synchronize_member(&mut self) {
        while !self.at_eof()
            && !self.at_symbol(TokSymbol::Semi)
            && !self.at_symbol(TokSymbol::RBrace)
        {
            self.bump();
        }
        self.eat_symbol(TokSymbol::Semi);
    }
}

fn split_variant_path(path: &Path) -> (Path, String) {
    let mut enum_segments = path.kind.segments.clone();
    let variant = enum_segments.pop().expect("variant path segment");
    (
        Node::new(
            path.span,
            PathKind {
                segments: enum_segments,
            },
        ),
        variant,
    )
}
