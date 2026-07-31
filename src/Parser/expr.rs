use std::{dbg, matches};

use super::*;

use crate::Ir::expr::*;

impl<'a> Parser<'a> {
    fn parse_struct_expr(&mut self, struct_name: &String) -> Expr {
        let mut fields = Vec::new();

        self.expect(TokenType::OpenScope);

        while self.peek(0).token != TokenType::CloseScope {
            let name_token = self.consume();
            if name_token.token != TokenType::Var {
                println!("name_token: {:?}", name_token);
                self::panic!("expected field name in struct init");
            }

            let field_name = name_token.value.unwrap();
            self.expect(TokenType::Colon);
            let value = self.parse_expr();

            fields.push((field_name, value));
            if self.peek(0).token != TokenType::CloseScope {
                self.expect(TokenType::Coma);
            }
        }
        self.expect(TokenType::CloseScope);
        let expr_ty = ExprType::StructInit {
            fields,
            struct_name_ty: struct_name.clone(),
        };
        Expr {
            ty: expr_ty,
            file: self.current_file.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn parse_init_array(&mut self) -> Expr {
        let mut elements = Vec::new();

        while self.peek(0).token != TokenType::CloseScope {
            elements.push(self.parse_expr());
            if self.peek(0).token == TokenType::Coma {
                self.consume();
            }
        }

        self.expect(TokenType::CloseScope);

        let expr_ty = ExprType::ArrayInit { elements };
        Expr {
            ty: expr_ty,
            file: self.current_file.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn parse_primary(&mut self) -> Expr {
        let token = self.consume();
        match token.token {
            TokenType::Var => {
                let token_value = token.value.unwrap();
                if self.is_struct(&token_value) && self.peek(0).token == TokenType::OpenScope {
                    self.parse_struct_expr(&token_value)
                } else {
                    let expr_ty = ExprType::Variable(token_value);
                    Expr {
                        ty: expr_ty,
                        file: self.current_file.clone(),
                        line: self.line,
                        col: self.col,
                    }
                }
            }

            TokenType::Num => {
                let expr_ty = ExprType::Number(token.value.unwrap().parse().unwrap());
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::HexNum => {
                let token_value = &token.value.unwrap();
                let expr_ty = ExprType::Number(i64::from_str_radix(token_value, 16).unwrap());
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::OctalNum => {
                let token_value = &token.value.unwrap();

                let expr_ty = ExprType::Number(i64::from_str_radix(token_value, 8).unwrap());
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::Mul => {
                let rhs = self.parse_primary();
                let expr_ty = ExprType::Deref(Box::new(rhs));
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }

            TokenType::Not => {
                let rhs = self.parse_primary();
                let expr_ty = ExprType::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(rhs),
                };
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::Sub => {
                let rhs = self.parse_primary();
                let expr_ty = ExprType::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(rhs),
                };
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }

            TokenType::OpenScope => self.parse_init_array(),

            TokenType::CharValue => {
                let s: i64 = token.value.unwrap().parse().unwrap();
                let expr_ty = ExprType::Number(s);
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }

            TokenType::SizeOf => {
                self.expect(TokenType::OpenParen);

                // Use your unified type parsing logic!
                let pre_ptr = self.parse_ptr();
                let base_ty = self.get_type();
                let mut ty = self.parse_generic_types(base_ty);
                let post_ptr = self.parse_ptr();

                ty = self.apply_ptr(ty, pre_ptr + post_ptr);
                ty = self.parse_array(ty);

                self.expect(TokenType::CloseParen);

                // Store the actual Type instead of a Stmt
                let expr_ty = ExprType::SizeOf { ty };

                return Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                };
            }

            TokenType::String => {
                let str_value = token.value.unwrap();
                let expr_ty = ExprType::String { str: str_value };
                return Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                };
            }

            TokenType::OpenParen => {
                let expr = self.parse_expr();
                self.expect(TokenType::CloseParen);
                return expr;
            }

            _ => self::panic!(
                "Unexpected token in primary expression: {:?}\n{:?}",
                token.token,
                self.m_tokens
            ),
        }
    }

    fn expr_to_ident(&self, expr: Expr) -> String {
        match expr.ty {
            ExprType::Variable(var) => var,
            _ => self::panic!("in expr_to_ident got wrong type of expr: {:?}", expr),
        }
    }

    fn precedence(op: &BinOp) -> u8 {
        match op {
            BinOp::Mul | BinOp::Div | BinOp::Mod => 10,
            BinOp::Add | BinOp::Sub => 8,
            BinOp::ShiftLeft | BinOp::ShiftRight => 7,
            BinOp::BitAnd => 6,
            BinOp::BitXor => 5,
            BinOp::BitOr => 4,
            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => 3,
            BinOp::Eq | BinOp::Neq => 3,
            BinOp::And => 2,
            BinOp::Or => 1,
        }
    }

    fn parse_unary(&mut self) -> Expr {
        match self.peek(0).token {
            TokenType::Mul => {
                self.consume();
                let rhs = self.parse_unary();
                let expr_ty = ExprType::Deref(Box::new(rhs));
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::Address => {
                self.consume();
                let rhs = self.parse_unary();
                let expr_ty = ExprType::Unary {
                    op: UnaryOp::GetAddr,
                    expr: Box::new(rhs),
                };
                return Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                };
            }
            TokenType::Sub => {
                self.consume();
                let rhs = self.parse_unary();
                let expr_ty = ExprType::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(rhs),
                };
                Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                }
            }
            TokenType::Not => {
                self.consume();
                let rhs = self.parse_unary();
                let expr_ty = ExprType::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(rhs),
                };
                return Expr {
                    ty: expr_ty,
                    file: self.current_file.clone(),
                    line: self.line,
                    col: self.col,
                };
            }
            _ => self.parse_postfix_chain(),
        }
    }

    fn parse_enum_expr_field(&mut self) -> EnumExprField {
        let name = self.consume().value.unwrap();
        self.expect(TokenType::Colon);
        let expr = self.parse_expr();
        if self.peek(0).token == TokenType::Coma {
            self.expect(TokenType::Coma);
        }
        return EnumExprField { name, expr };
    }

    pub fn parse_postfix_chain(&mut self) -> Expr {
        let mut expr: Expr = self.parse_primary();
        loop {
            match self.peek(0).token {
                TokenType::OpenBracket => {
                    self.consume();
                    let index = self.parse_expr();
                    self.expect(TokenType::CloseBracket);
                    let expr_ty = ExprType::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    };
                    expr = Expr {
                        ty: expr_ty,
                        file: self.current_file.clone(),
                        line: self.line,
                        col: self.col,
                    };
                }

                TokenType::As => {
                    self.consume();
                    let pre_ptr = self.parse_ptr();
                    let ty = self.get_type();
                    let post_ptr = self.parse_ptr();
                    let ty = self.apply_ptr(ty, pre_ptr + post_ptr);
                    let expr_ty = ExprType::Cast {
                        expr: Box::new(expr),
                        ty,
                    };
                    expr = Expr {
                        ty: expr_ty,
                        file: self.current_file.clone(),
                        line: self.line,
                        col: self.col,
                    };
                }

                TokenType::Dot => {
                    self.expect(TokenType::Dot);
                    if self.peek(1).token == TokenType::OpenParen
                        || self.peek(1).token == TokenType::Less
                    {
                        // vec.vector_push(5) syntax
                        let name_ident = self.parse_primary(); // just consumes the function name token

                        let call = if self.peek(0).token == TokenType::Less {
                            self.parse_generic_call(name_ident)
                        } else {
                            self.parse_call(name_ident)
                        };

                        match call.ty {
                            ExprType::Call {
                                name,
                                generics,
                                args,
                            } => {
                                let mut new_args = args.clone();
                                new_args.insert(0, expr);
                                let expr_ty = ExprType::Call {
                                    generics: generics,
                                    name: name,
                                    args: new_args,
                                };
                                expr = Expr {
                                    ty: expr_ty,
                                    file: self.current_file.clone(),
                                    line: self.line,
                                    col: self.col,
                                };
                            }
                            _ => self::panic!(""),
                        }
                    } else {
                        // vector_push(vec,5) syntax
                        let name = self.consume().value.unwrap();
                        let expr_ty = ExprType::StructMember {
                            base: Box::new(expr),
                            name: name,
                        };
                        expr = Expr {
                            ty: expr_ty,
                            file: self.current_file.clone(),
                            line: self.line,
                            col: self.col,
                        };
                    }
                }

                TokenType::Access => {
                    self.consume();
                    let name = self.consume().value.unwrap();
                    let base = Box::new(Expr {
                        ty: ExprType::Deref(Box::new(expr)),
                        file: self.current_file.clone(),
                        line: self.line,
                        col: self.col,
                    });
                    let expr_ty = ExprType::StructMember { base, name: name };
                    expr = Expr {
                        ty: expr_ty,
                        file: self.current_file.clone(),
                        line: self.line,
                        col: self.col,
                    };
                }

                TokenType::Less => {
                    if self.is_type(self.peek(1)) {
                        expr = self.parse_generic_call(expr);
                    } else {
                        break;
                    }
                }

                TokenType::OpenParen => {
                    expr = self.parse_call(expr);
                }

                TokenType::Colon => {
                    self.expect(TokenType::Colon);
                    self.expect(TokenType::Colon);
                    let value_name = self.consume().value.unwrap();
                    let mut variant_expr: Vec<EnumExprField> = Vec::new();
                    if self.peek(0).token == TokenType::OpenParen {
                        self.expect(TokenType::OpenParen);
                        while self.peek(0).token != TokenType::CloseParen {
                            let res = self.parse_enum_expr_field();
                            variant_expr.push(res);
                        }
                        self.expect(TokenType::CloseParen);
                    }
                    match &expr.ty {
                        ExprType::Variable(name) => {
                            let expr_ty = ExprType::GetEnum {
                                base: name.to_string(),
                                variant: value_name,
                                value: variant_expr,
                            };

                            expr = Expr {
                                ty: expr_ty,
                                file: self.current_file.clone(),
                                line: self.line,
                                col: self.col,
                            }
                        }
                        _ => self::panic!("really strange syntax error"),
                    }
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_generic_call(&mut self, expr: Expr) -> Expr {
        let mut generics = Vec::new();
        self.expect(TokenType::Less);
        while self.peek(0).token != TokenType::More {
            let bef_ptr = self.parse_ptr();
            let ty = self.get_type();
            let aft_ptr = self.parse_ptr();
            let ty = self.parse_array(ty);
            let ty = self.apply_ptr(ty, bef_ptr + aft_ptr);
            generics.push(ty);
            if self.peek(0).token == TokenType::Coma {
                self.consume();
            }
        }
        self.expect(TokenType::More);
        let mut args: Vec<Expr> = Vec::new();
        if self.peek(0).token != TokenType::CloseParen {
            self.expect(TokenType::OpenParen);
            loop {
                if self.peek(0).token == TokenType::CloseParen {
                    break;
                }
                args.push(self.parse_expr());
                if self.peek(0).token == TokenType::Coma {
                    self.expect(TokenType::Coma);
                }
            }
        }
        self.expect(TokenType::CloseParen);
        let expr_ty = ExprType::Call {
            generics,
            name: self.expr_to_ident(expr),
            args,
        };
        Expr {
            ty: expr_ty,
            file: self.current_file.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn parse_call(&mut self, expr: Expr) -> Expr {
        self.expect(TokenType::OpenParen);
        let mut args: Vec<Expr> = Vec::new();
        if self.peek(0).token != TokenType::CloseParen {
            loop {
                args.push(self.parse_expr());
                if self.peek(0).token == TokenType::CloseParen {
                    break;
                }
                self.expect(TokenType::Coma);
            }
        }
        self.expect(TokenType::CloseParen);
        let expr_ty = ExprType::Call {
            generics: Vec::new(),
            name: self.expr_to_ident(expr),
            args,
        };
        Expr {
            ty: expr_ty,
            file: self.current_file.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn is_bin_op(ty: TokenType) -> Option<BinOp> {
        match ty {
            TokenType::Add => Some(BinOp::Add),
            TokenType::Sub => Some(BinOp::Sub),
            TokenType::Mul => Some(BinOp::Mul),
            TokenType::Div => Some(BinOp::Div),
            TokenType::AsertEq => Some(BinOp::Eq),
            TokenType::NotEq => Some(BinOp::Neq),
            TokenType::LessThan => Some(BinOp::Lte),
            TokenType::Less => Some(BinOp::Lt),
            TokenType::More => Some(BinOp::Gt),
            TokenType::MoreThan => Some(BinOp::Gte),
            TokenType::And => Some(BinOp::And),
            TokenType::Or => Some(BinOp::Or),
            TokenType::Remainder => Some(BinOp::Mod),
            TokenType::Address => Some(BinOp::BitAnd),
            TokenType::BitOr => Some(BinOp::BitOr),
            TokenType::BitXor => Some(BinOp::BitXor),
            TokenType::LeftShift => Some(BinOp::ShiftLeft),
            TokenType::RightShift => Some(BinOp::ShiftRight),
            _ => None,
        }
    }

    pub fn parse_expr(&mut self) -> Expr {
        self.parse_binary(0)
    }

    fn parse_binary(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary();
        if self.peek(0).token == TokenType::Semi {
            return left;
        }
        loop {
            let op = match Parser::is_bin_op(self.peek(0).token) {
                Some(op) => op,
                None => break,
            };

            let prec = Parser::precedence(&op);

            if prec < min_prec {
                break;
            }

            self.consume();

            let right = self.parse_binary(prec + 1);

            let bin_ty = ExprType::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };

            left = Expr {
                ty: bin_ty,
                file: self.current_file.clone(),
                line: self.line,
                col: self.col,
            };
        }

        left
    }
}
