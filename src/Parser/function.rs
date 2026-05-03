use crate::Ir::stmt::Declaration;

use super::*;

impl<'a> Parser<'a> {
    pub fn parse_args(&mut self) -> Vec<Declaration> {
        let mut res: Vec<Declaration> = Vec::new();
        self.consume(); // ( 
        while self.peek(0).token != TokenType::CloseParen {
            let arg = self.parse_declaration().unwrap();
            match arg {
                Stmt::Declaration(decl) => {
                    res.push(decl);
                }
                _ => panic!("wrong args"),
            }
            if self.peek(0).token == TokenType::Coma {
                self.consume();
            }
        }
        self.consume();
        res
    }

    pub fn parse_func_init(&mut self) -> Option<Stmt> {
        self.consume(); //keyword
        let name = self.consume().value.unwrap();
        let generics = self.parse_generic();
        let args = self.parse_args();
        let mut ret_type = Type::Primitive(TokenType::Void);
        if self.peek(0).token == TokenType::Access {
            self.consume();
            let ty = self.get_type();
            let ty = self.parse_ptr(ty);
            let ty = self.parse_array(ty);
            let ty = self.parse_generic_types(ty);

            ret_type = ty;
        }
        let data = Box::new(
            self.parse_stmt()
                .expect(&format!("the func: {} is empty", name)),
        );
        return Some(Stmt::InitFunc {
            generic_types: generics,
            name,
            args,
            ret_type,
            data,
        });
    }
}
