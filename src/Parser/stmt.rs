use std::fs::File;
use std::io::Read;

use super::*;

use crate::Ir::expr::Expr;
use crate::Ir::stmt::*;
use crate::Tokenizer;

impl<'a> Parser<'a> {
    pub fn parse_stmt(&mut self) -> Option<Stmt> {
        let token = self.peek(0);
        match token.token {
            TokenType::If => return self.parse_if(),
            TokenType::While => return self.parse_while(),
            TokenType::For => return self.parse_for(),
            TokenType::OpenScope => return self.parse_scope(),
            TokenType::Return => return self.parse_ret(),
            TokenType::Asm => return self.parse_asm_stmt(),
            TokenType::Func => return self.parse_func_init(),
            TokenType::Struct => return self.parse_struct_init(),
            TokenType::Global => return self.parse_global(),
            TokenType::Import => {
                return {
                    self.parse_import();

                    // the import shouldnt return any stmt
                    // but we need to return something to satisfy the func needs refactor
                    Some(Stmt::Block(Vec::new()))
                };
            }
            ty if self.is_type(&token) => {
                let stmt = self.parse_declaration();
                self.expect(TokenType::Semi);
                return stmt;
            }
            _ => {
                if self.check_assignment_start() {
                    return self.parse_assignment();
                }
                return self.parse_expr_stmt();
            }
        };
    }

    fn parse_global(&mut self) -> Option<Stmt> {
        self.consume();
        let stmt = self.parse_declaration().unwrap();
        self.expect(TokenType::Semi);
        return Some(Stmt::GlobalDecl(Box::new(stmt)));
    }

    pub fn is_type(&self, token: &Token) -> bool {
        match token.token {
            TokenType::IntType => true,
            TokenType::CharType => true,
            TokenType::LongType => true,
            TokenType::ShortType => true,
            TokenType::Void => true,
            TokenType::Var => {
                if let Some(name) = &token.value {
                    self.types.contains(name)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub fn check_ptr(&mut self) -> usize {
        let mut index = 0;
        while self.m_index < self.m_tokens.len() && self.peek(0).token == TokenType::Mul {
            index += 1;
        }
        index
    }

    pub fn parse_ptr(&mut self, mut ty: Type) -> Type {
        while self.m_index < self.m_tokens.len() && self.peek(0).token == TokenType::Mul {
            self.consume();
            ty = Type::Pointer(Box::new(ty));
        }
        ty
    }
    pub fn parse_array(&mut self, mut ty: Type) -> Type {
        while self.m_index < self.m_tokens.len() && self.peek(0).token == TokenType::OpenBracket {
            self.consume();
            let size_token = self.consume();
            let mut size = 0;
            if size_token.token != TokenType::CloseBracket {
                size = size_token.value.unwrap().parse::<usize>().unwrap();
                self.expect(TokenType::CloseBracket);
            }
            ty = Type::Array(Box::new(ty), size);
        }
        ty
    }

    pub fn get_type(&mut self) -> Type {
        let mut pointer_depth = 0;

        while self.peek(0).token == TokenType::Mul {
            self.consume();
            pointer_depth += 1;
        }

        let token = self.consume();

        let mut ty = if token.token == TokenType::Var {
            let name = self.types.get(&token.value.unwrap()).unwrap();
            Type::Struct(name.to_string())
        } else {
            Type::Primitive(token.token)
        };
        for _ in 0..pointer_depth {
            ty = Type::Pointer(Box::new(ty));
        }

        ty
    }

    pub fn parse_declaration(&mut self) -> Option<Stmt> {
        let ty = self.get_type();
        let ty = self.parse_ptr(ty);
        let var_name = self.consume();
        let mut ty = self.parse_array(ty);




        let mut expr: Option<Expr> = None;
        if self.peek(0).token == TokenType::Eq {
            self.consume();
            let initializer = self.parse_expr();
            
            // fix up char[] size from string literal
            if let (Type::Array(inner, 0), Expr::String{str: s}) = (&ty, &initializer) {
                if **inner == Type::Primitive(TokenType::CharType) {
                    ty = Type::Array(
                        Box::new(Type::Primitive(TokenType::CharType)),
                        s.len() + 1
                    );
                }
            }
            
            expr = Some(initializer);
        }
        return Some(Stmt::Declaration(Declaration {
            name: var_name.value.unwrap(),
            ty: ty,
            initializer: expr,
        }));
    }

    fn parse_import(&mut self) {
        self.consume(); // consume 'import' keyword
        let file_name = self.consume().value.unwrap();
        let full_path = self
            .base_dir
            .join(&file_name)
            .canonicalize()
            .expect(&format!("Cannot find import: {}", file_name));

        // check canonical path, not the raw string
        let canonical_str = full_path.to_str().unwrap().to_string();
        if self.imported_files.contains(&canonical_str) {
            return;
        }
        self.imported_files.insert(canonical_str);

        let mut file = File::open(&full_path).expect(&format!("Cannot find import: {}", file_name));

        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();

        let mut tokenizer = Tokenizer::Tokenizer::new(content);
        tokenizer.tokenize();

        let mut parser = Parser::new(tokenizer.m_res, self.base_dir.clone(), self.imported_files);
        parser.base_dir = full_path.parent().unwrap().to_path_buf();
        let imported_stmts = parser.parse();
        self.types.extend(parser.types);
        self.struct_table.extend(parser.struct_table);

        for stmt in imported_stmts {
            self.expressions.push(stmt);
        }
    }

    fn parse_struct_init(&mut self) -> Option<Stmt> {
        self.consume(); // 'struct'

        let struct_name = self.consume().value.unwrap();

        self.expect(TokenType::OpenScope);

        let mut fields: Vec<StructField> = Vec::new();
        let mut offset: usize = 0;

        while self.peek(0).token != TokenType::CloseScope {
            let base_token = self.consume();

            let mut ty = if self.is_type(&base_token) && base_token.token != TokenType::Var {
                Type::Primitive(base_token.token)
            } else if self.is_type(&base_token) && base_token.token == TokenType::Var {
                Type::Struct(base_token.value.unwrap())
            } else {
                panic!("Expected type in struct field");
            };

            ty = self.parse_ptr(ty);
            let field_name = self.consume().value.unwrap();

            ty = self.parse_array(ty);

            self.expect(TokenType::Semi);

            let field_size = self.size_of(&ty);

            fields.push(StructField {
                name: field_name,
                ty: ty.clone(),
                offset,
            });

            offset += field_size;
        }

        self.expect(TokenType::CloseScope);

        let struct_size = offset;

        let def = StructDef {
            name: struct_name.clone(),
            fields,
            size: struct_size,
        };

        // register struct in type table
        self.types.insert(struct_name.clone());
        self.struct_table.insert(struct_name.clone(), def.clone());

        Some(Stmt::InitStruct(def))
    }

    fn check_assignment_start(&self) -> bool {
        let mut i = 0;

        // allow leading *
        while self.peek(i).token == TokenType::Mul {
            i += 1;
        }

        // must start with identifier
        if self.peek(i).token != TokenType::Var {
            return false;
        }

        i += 1;

        loop {
            match self.peek(i).token {
                TokenType::Dot => {
                    i += 1;
                    if self.peek(i).token != TokenType::Var {
                        return false;
                    }
                    i += 1;
                }
                TokenType::Access => {
                    i += 1;
                    if self.peek(i).token != TokenType::Var {
                        return false;
                    }
                    i += 1;
                }

                TokenType::OpenBracket => {
                    i += 1;

                    // skip until closing bracket
                    let mut depth = 1;
                    while depth > 0 {
                        match self.peek(i).token {
                            TokenType::OpenBracket => depth += 1,
                            TokenType::CloseBracket => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                }

                TokenType::Eq => {
                    return true;
                }

                _ => {
                    return false;
                }
            }
        }
    }

    fn parse_assignment_no_semi(&mut self) -> Option<Stmt> {
        let mut pointer_depth = 0;
        while self.peek(0).token == TokenType::Mul {
            self.consume();
            pointer_depth += 1;
        }

        let var_name = self.consume().value.unwrap();
        let mut lvalue = LValue::Variable(var_name);

        loop {
            match self.peek(0).token {
                TokenType::Dot => {
                    self.consume();
                    let field = self.consume().value.unwrap();
                    lvalue = LValue::Field {
                        base: Box::new(lvalue),
                        name: field,
                    };
                }
                TokenType::Access => {
                    self.consume();
                    let field = self.consume().value.unwrap();
                    lvalue = LValue::Field {
                        base: Box::new(LValue::Deref(Box::new(lvalue))),
                        name: field,
                    };
                }

                TokenType::OpenBracket => {
                    self.consume();
                    let index = self.parse_expr();
                    self.expect(TokenType::CloseBracket);

                    lvalue = LValue::Index {
                        base: Box::new(lvalue),
                        index: Box::new(index),
                    };
                }

                _ => break,
            }
        }

        for _ in 0..pointer_depth {
            lvalue = LValue::Deref(Box::new(lvalue));
        }
        self.expect(TokenType::Eq);

        let value = self.parse_expr();

        Some(Stmt::Assignment {
            target: lvalue,
            value,
        })
    }

    fn parse_assignment(&mut self) -> Option<Stmt> {
        let res = self.parse_assignment_no_semi();
        self.expect(TokenType::Semi);
        return res;
    }

    fn parse_scope(&mut self) -> Option<Stmt> {
        self.consume();
        let mut stmts: Vec<Stmt> = Vec::new();
        while self.peek(0).token != TokenType::CloseScope {
            let stmt = self.parse_stmt().unwrap();
            stmts.push(stmt);
        }
        self.consume();
        return Some(Stmt::Block(stmts));
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        self.consume();
        let condition = self.parse_expr();
        let if_block = Box::new(self.parse_stmt().unwrap()); // should be block
        let mut else_block: Option<Box<Stmt>> = None;
        if self.peek(0).token == TokenType::Else {
            self.consume();
            let else_data = Box::new(self.parse_stmt().unwrap());
            else_block = Some(else_data);
        }
        return Some(Stmt::If {
            condition,
            if_block,
            else_block,
        });
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        self.consume();
        let condition = self.parse_expr();
        let body = Box::new(self.parse_stmt().unwrap());
        return Some(Stmt::While { condition, body });
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        self.consume(); // the keyword itself
        self.consume(); // (

        let init = if self.is_type(&self.peek(0)) {
            Some(Box::new(self.parse_declaration().unwrap()))
        } else {
            None
        };
        self.expect(TokenType::Semi);
        let condition = if self.peek(0).token != TokenType::Semi {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenType::Semi);

        let update = if self.peek(0).token != TokenType::CloseParen {
            Some(Box::new(self.parse_assignment_no_semi().unwrap()))
        } else {
            None
        };

        self.consume(); // )
        let body = Box::new(self.parse_stmt().unwrap());
        return Some(Stmt::For {
            init,
            condition,
            update,
            body,
        });
    }

    fn parse_asm_stmt(&mut self) -> Option<Stmt> {
        self.consume(); // the keyword itself
        let mut asm_code: Vec<String> = Vec::new();
        self.consume();
        while self.peek(0).token != TokenType::CloseScope {
            let str = self.consume();
            asm_code.push(str.value.unwrap());
        }
        self.consume();
        return Some(Stmt::AsmCode(asm_code));
    }

    fn parse_ret(&mut self) -> Option<Stmt> {
        self.consume(); // the keyword itself
        let expr = if self.peek(0).token != TokenType::Semi {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenType::Semi);
        Some(Stmt::Return(expr))
    }

    fn parse_expr_stmt(&mut self) -> Option<Stmt> {
        let expr = self.parse_expr();
        self.expect(TokenType::Semi);
        Some(Stmt::ExprStmt(expr))
    }
}

// #[test]
// fn test_single_pointer() {
//     let tokens = vec![Token {
//         token: TokenType::Mul,
//         value: None,
//     }];

//     let mut parser = Parser {
//         m_tokens: tokens,
//         m_index: 0,
//         struct_table: HashMap::new(),
//         expressions: Vec::new(),
//         types: HashSet::new(),
//         base_dir: PathBuf::new(),
//     };

//     let result = parser.parse_ptr(Type::Primitive(TokenType::IntType));
//     println!("result: {:?}", result);
//     assert_eq!(
//         result,
//         Type::Pointer(Box::new(Type::Primitive(TokenType::IntType)))
//     );
// }

// #[test]
// fn test_double_pointer() {
//     let tokens = vec![
//         Token {
//             token: TokenType::Mul,
//             value: None,
//         },
//         Token {
//             token: TokenType::Mul,
//             value: None,
//         },
//     ];

//     let mut parser = Parser {
//         m_tokens: tokens,
//         m_index: 0,
//         struct_table: HashMap::new(),
//         expressions: Vec::new(),
//         types: HashSet::new(),
//         base_dir: PathBuf::new(),
//     };

//     let result = parser.parse_ptr(Type::Primitive(TokenType::IntType));
//     println!("result: {:?}", result);
//     assert_eq!(
//         result,
//         Type::Pointer(Box::new(Type::Pointer(Box::new(Type::Primitive(
//             TokenType::IntType
//         )))))
//     );
// }

// #[test]
// fn test_array_simple() {
//     let tokens = vec![
//         Token {
//             token: TokenType::OpenBracket,
//             value: None,
//         },
//         Token {
//             token: TokenType::Num,
//             value: Some("5".to_string()),
//         },
//         Token {
//             token: TokenType::CloseBracket,
//             value: None,
//         },
//     ];

//     let mut parser = Parser {
//         m_tokens: tokens,
//         m_index: 0,
//         struct_table: HashMap::new(),
//         expressions: Vec::new(),
//         types: HashSet::new(),
//         base_dir: PathBuf::new(),
//     };

//     let result = parser.parse_array(Type::Primitive(TokenType::IntType));

//     assert_eq!(
//         result,
//         Type::Array(Box::new(Type::Primitive(TokenType::IntType)), 5)
//     );
// }

// #[test]
// fn test_pointer_array() {
//     let tokens = vec![
//         // for parse_ptr (pointer)
//         Token {
//             token: TokenType::Mul,
//             value: None,
//         },
//         // for parse_array
//         Token {
//             token: TokenType::OpenBracket,
//             value: None,
//         },
//         Token {
//             token: TokenType::Num,
//             value: Some("5".to_string()),
//         },
//         Token {
//             token: TokenType::CloseBracket,
//             value: None,
//         },
//     ];

//     let mut parser = Parser {
//         m_tokens: tokens,
//         struct_table: HashMap::new(),
//         m_index: 0,
//         expressions: Vec::new(),
//         types: HashSet::new(),
//         base_dir: PathBuf::new(),
//     };

//     // First parse pointer
//     let ty = parser.parse_ptr(Type::Primitive(TokenType::IntType));

//     // Then parse array
//     let result = parser.parse_array(ty);

//     assert_eq!(
//         result,
//         Type::Array(
//             Box::new(Type::Pointer(Box::new(Type::Primitive(TokenType::IntType)))),
//             5
//         )
//     );
// }
