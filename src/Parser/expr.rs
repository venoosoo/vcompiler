use super::*;


use crate::Ir::expr::*;

impl Parser {


    fn parse_struct_expr(&mut self) -> Expr {
        let mut fields = Vec::new();

        while self.peek(0).token != TokenType::CloseScope {
            let name_token = self.consume();
            if name_token.token != TokenType::Var {
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
        Expr::StructInit { fields, struct_name: None }
    }

    fn parse_primary(&mut self) -> Expr {
        let token = self.consume();
        match token.token {
            TokenType::Var => Expr::Variable(token.value.unwrap()),
            TokenType::Num => Expr::Number(token.value.unwrap().parse().unwrap()),
            TokenType::Mul => {
                let rhs = self.parse_primary();
                Expr::Deref(Box::new(rhs))
            },
            TokenType::Address => {
                let rhs = self.parse_primary();
                Expr::AddressOf(Box::new(rhs))
            },
            TokenType::OpenParen => {
                let expr = self.parse_expr();
                self.expect(TokenType::CloseParen);
                expr
            }
            TokenType::Not => {
                let rhs = self.parse_primary();
                Expr::Unary { op: UnaryOp::Not, expr: Box::new(rhs) }
            }
            TokenType::Sub => {
                let rhs = self.parse_primary();
                Expr::Unary { op: UnaryOp::Neg, expr: Box::new(rhs) }
            }

            TokenType::OpenScope => {
                self.parse_struct_expr()
            }

            TokenType::CharValue => {
                let s: i64 = token.value.unwrap().parse().unwrap();
                Expr::Number(s as i64)
            }
            

            _ => panic!("Unexpected token in primary expression: {:?}\n{:?}", token.token,self.m_tokens),
        }
    }

    fn expr_to_ident(&self, expr: Expr) -> String {
        match expr {
            Expr::Variable(var) => {
                var
            }
            _ => panic!("in expr_to_ident go wrong type of expr: {:?}",expr)
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
                Expr::Unary { op: UnaryOp::Neg, expr: Box::new(rhs) }
            }
            TokenType::Not => {
                self.consume();
                let rhs = self.parse_unary();
                Expr::Unary { op: UnaryOp::Not, expr: Box::new(rhs) }
            }
            _ => self.parse_postfix_chain(),
        }
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
                TokenType::Dot => {
                    self.consume();
                    let name = self.consume().value.unwrap();
                    expr = Expr::StructMember {
                        base: Box::new(expr),
                        name,
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
            TokenType::Eq => Some(BinOp::Eq),
            TokenType::NotEq => Some(BinOp::Neq),
            TokenType::LessThan => Some(BinOp::Lte),
            TokenType::Less => Some(BinOp::Lt),
            TokenType::More => Some(BinOp::Gt),
            TokenType::MoreThan => Some(BinOp::Gte),
            TokenType::And => Some(BinOp::And),
            TokenType::Or => Some(BinOp::Or),
            TokenType::Remainder => Some(BinOp::Mod),
            _ => None
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
