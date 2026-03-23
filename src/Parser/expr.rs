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
                panic!("expected field name in struct init");
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
        Expr::StructInit {
            fields,
            struct_name_ty: struct_name.clone(),
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
        Expr::ArrayInit { elements }
    }

    fn parse_primary(&mut self) -> Expr {
        let token = self.consume();
        match token.token {
            TokenType::Var => {
                let token_value = token.value.unwrap();
                if self.is_struct(&token_value) && self.peek(0).token == TokenType::OpenScope {
                    self.parse_struct_expr(&token_value)
                } else {
                    Expr::Variable(token_value)
                }
            }

            TokenType::Num => Expr::Number(token.value.unwrap().parse().unwrap()),
            TokenType::Mul => {
                let rhs = self.parse_primary();
                Expr::Deref(Box::new(rhs))
            }
            TokenType::Address => {
                let rhs = self.parse_primary();
                Expr::AddressOf(Box::new(rhs))
            }

            TokenType::OpenParen => {
                let expr = self.parse_expr();
                self.expect(TokenType::CloseParen);
                expr
            }

            TokenType::Not => {
                let rhs = self.parse_primary();
                Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(rhs),
                }
            }
            TokenType::Sub => {
                let rhs = self.parse_primary();
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(rhs),
                }
            }

            TokenType::OpenScope => self.parse_init_array(),

            TokenType::CharValue => {
                let s: i64 = token.value.unwrap().parse().unwrap();
                Expr::Number(s as i64)
            }

            TokenType::SizeOf => {
                self.expect(TokenType::OpenParen);

                // the stmt will be declaration with name ) and right type

                let stmt = self.parse_stmt().unwrap();

                self.expect(TokenType::Semi);
                return Expr::SizeOf { ty: Box::new(stmt) };
            }

            TokenType::String => {
                let str_value = token.value.unwrap();
                return Expr::String { str: str_value };
            }

            _ => panic!(
                "Unexpected token in primary expression: {:?}\n{:?}",
                token.token, self.m_tokens
            ),
        }
    }

    fn expr_to_ident(&self, expr: Expr) -> String {
        match expr {
            Expr::Variable(var) => var,
            _ => panic!("in expr_to_ident go wrong type of expr: {:?}", expr),
        }
    }

    fn precedence(op: &BinOp) -> u8 {
        match op {
            BinOp::Mul | BinOp::Div | BinOp::Mod => 5,
            BinOp::Add | BinOp::Sub => 4,
            BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => 3,
            BinOp::Eq | BinOp::Neq => 2,
            BinOp::And => 1,
            BinOp::Or => 0,
        }
    }

    fn parse_unary(&mut self) -> Expr {
        match self.peek(0).token {
            TokenType::Mul => {
                self.consume();
                let rhs = self.parse_unary();
                Expr::Deref(Box::new(rhs))
            }
            TokenType::Address => {
                self.consume();
                let rhs = self.parse_unary();
                Expr::AddressOf(Box::new(rhs))
            }
            TokenType::Sub => {
                self.consume();
                let rhs = self.parse_unary();
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(rhs),
                }
            }
            TokenType::Not => {
                self.consume();
                let rhs = self.parse_unary();
                Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(rhs),
                }
            }
            _ => self.parse_postfix_chain(),
        }
    }

    fn parse_enum_expr_field(&mut self) -> EnumExprField {
        let name = self.consume().value.unwrap();
        self.expect(TokenType::Colon);
        let expr = self.parse_expr();
        self.expect(TokenType::Coma);
        return EnumExprField { name, expr };
    }

    pub fn parse_postfix_chain(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            match self.peek(0).token {
                TokenType::OpenBracket => {
                    self.consume();
                    let index = self.parse_expr();
                    self.expect(TokenType::CloseBracket);
                    expr = Expr::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    };
                }

                TokenType::As => {
                    self.consume();
                    let mut pointer_depth = 0;
                    while self.peek(0).token == TokenType::Mul {
                        self.consume();
                        pointer_depth += 1;
                    }
                    let ty = self.get_type();
                    let mut ty = self.parse_ptr(ty);
                    for _ in 1..pointer_depth {
                        ty = Type::Pointer(Box::new(ty));
                    }
                    return Expr::Cast {
                        expr: Box::new(expr),
                        ty,
                    };
                }

                TokenType::Dot => {
                    self.consume();
                    let name = self.consume().value.unwrap();
                    expr = Expr::StructMember {
                        base: Box::new(expr),
                        name,
                    };
                }

                TokenType::Access => {
                    self.consume();
                    let name = self.consume().value.unwrap();
                    return Expr::StructMember {
                        base: Box::new(Expr::Deref(Box::new(expr))),
                        name: name,
                    };
                }

                TokenType::OpenParen => {
                    self.consume();
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
                    expr = Expr::Call {
                        name: self.expr_to_ident(expr),
                        args,
                    };
                }

                TokenType::Colon => {
                    self.consume();
                    self.expect(TokenType::Colon);
                    let value_name = self.consume().value.unwrap();
                    let mut variant_expr: HashMap<String, EnumExprField> = HashMap::new();
                    if self.peek(0).token == TokenType::OpenParen {
                        self.expect(TokenType::OpenParen);
                        while self.peek(0).token != TokenType::CloseParen {
                            let res = self.parse_enum_expr_field();
                            variant_expr.insert(res.name.clone(), res);
                        }
                        self.expect(TokenType::CloseParen);
                    }
                    self.expect(TokenType::Semi);
                    match expr {
                        Expr::Variable(name) => {
                            expr = Expr::GetEnum {
                                base: name,
                                variant: value_name,
                                value: variant_expr,
                            };
                        }
                        _ => panic!("really strange syntax error"),
                    }
                }
                _ => break,
            }
        }
        expr
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
            _ => None,
        }
    }

    pub fn parse_expr(&mut self) -> Expr {
        self.parse_binary(0)
    }

    fn parse_binary(&mut self, min_prec: u8) -> Expr {
        let mut left = self.parse_unary();
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

            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        left
    }
}
