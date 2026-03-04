use crate::Ir::expr::{BinOp, Expr, UnaryOp};
use crate::Ir::stmt::LValue;

use super::*;
use super::{reg_for_size, get_word};


pub struct RegisterHelper {
    reg_stack: Vec<String>,
}



impl Expr {
    /// Returns the Type of this expression
    pub fn get_type_of_expr(&self, gen_helper: &Gen) -> Type {
        match self {
            Expr::Number(_) => Type::Primitive(TokenType::IntType),
            Expr::Float(_) => self::panic!("floats are not implemented"),
            Expr::Variable(name) => {
                let var = gen_helper.lookup_var(name);
                var.var_type.clone()
            }
            Expr::Unary { op, expr } => {
                match op {
                    UnaryOp::Neg => expr.get_type_of_expr(gen_helper),
                    UnaryOp::Not => Type::Primitive(TokenType::CharType), // boolean
                }
            }
            Expr::Binary { op: _, left, right } => {
                let lty = left.get_type_of_expr(gen_helper);
                let rty = right.get_type_of_expr(gen_helper);
                if lty != rty {
                    self::panic!(
                        "Type mismatch in binary operation: left: {:?}, right: {:?}",
                        lty, rty
                    );
                }
                lty
            }
            Expr::StructInit { struct_name, fields: _ } => {
                if let Some(name) = struct_name {
                    // Check that this struct actually exists in our known structs
                    if let Some(_struct_data) = gen_helper.structs.get(name) {
                        Type::Struct(name.clone())
                    } else {
                        self::panic!("Struct {} not found in get_type_of_expr", name);
                    }
                } else {
                    self::panic!("StructInit expression missing struct name")
                }
            }
            
            Expr::Deref(ptr_expr) => {
                match ptr_expr.get_type_of_expr(gen_helper) {
                    Type::Pointer(inner) => *inner,
                    _ => self::panic!("Cannot dereference a non-pointer"),
                }
            }
            Expr::AddressOf(var_expr) => {
                let ty = var_expr.get_type_of_expr(gen_helper);
                Type::Pointer(Box::new(ty))
            }
            Expr::Index { base, index } => {
                let base_ty = base.get_type_of_expr(gen_helper);
                let idx_ty = index.get_type_of_expr(gen_helper);
                if idx_ty != Type::Primitive(TokenType::IntType) {
                    self::panic!("Array index must be integer");
                }
                match base_ty {
                    Type::Array(elem_ty, _) => *elem_ty,
                    Type::Pointer(elem_ty) => *elem_ty,
                    _ => self::panic!("Cannot index into non-array type"),
                }
            }
            Expr::StructMember { base, name } => {
                let base_ty = base.get_type_of_expr(gen_helper);
                match base_ty {
                    Type::Struct(ref struct_name) => {
                        let struct_data = gen_helper.structs.get(struct_name)
                            .expect(&format!("Struct {} not found", struct_name));
                        let field = struct_data.elements.get(name)
                            .expect(&format!("Field {} not found in struct {}", name, struct_name));
                        field.ty.clone()
                    }
                    _ => self::panic!("Cannot access member of non-struct type"),
                }
            }
            Expr::Call { name, args: _ } => {
                let func_data = gen_helper.functions.get(name)
                    .expect(&format!("Function {} not found", name));
                func_data.return_type.clone()
            }
        }
    }
}




impl RegisterHelper {
    pub fn insert_reg(&mut self,ty: &Type) -> String {
        if self.reg_stack.len() == 0 {
            let reg: String = reg_for_size("rax", ty);
            self.reg_stack.push(reg.clone());

            return reg;
        }
        else if self.reg_stack.len() == 1 {
            let reg: String = reg_for_size("rbx", ty);
            self.reg_stack.push(reg.clone());
            return reg;
        } else {
            println!("what: {:?}",self.reg_stack);
            self::panic!("in RegisterHelper insert_value tring to insert the 3 value");
        }
    }
    pub fn get_reg(&mut self) -> String {
        self.reg_stack.pop().expect("RegisterHelper get_reg trying to get register but its empty")
    }
}


impl Gen {



    fn gen_expr_binop(&mut self, op: &BinOp, left_reg: &str, right_reg: &str,expected_type: &Type) {
        match op {
            BinOp::Add => {
                self.emit(format!("    add {}, {}", left_reg, right_reg));
            }
            BinOp::Sub => {
                self.emit(format!("    sub {}, {}", left_reg, right_reg));
            }
            BinOp::Mul => {
                self.emit(format!("    imul {}, {}", left_reg, right_reg));
            }
            BinOp::Div | BinOp::Mod => {
                self.emit("    cdq".to_string());
                self.emit(format!("    idiv {}",right_reg));

                if let BinOp::Mod = op {
                    self.emit(format!("    mov {}, {}",left_reg,reg_for_size("rdx", expected_type)));
                }
            }
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.emit(format!("    cmp {}, {}", left_reg, right_reg));
                let set_instr = match op {
                    BinOp::Eq => "sete",
                    BinOp::Neq => "setne",
                    BinOp::Lt => "setl",
                    BinOp::Lte => "setle",
                    BinOp::Gt => "setg",
                    BinOp::Gte => "setge",
                    _ => unreachable!(),
                };
                self.emit(format!("    {} al", set_instr));
                self.emit(format!("    movzx {}, al", left_reg));
            }
            BinOp::And => {
                self.emit(format!("    cmp {}, 0", left_reg));
                self.emit(format!("    setne al"));        // al = left != 0
                self.emit(format!("    cmp {}, 0", right_reg));
                self.emit(format!("    setne dl"));        // dl = right != 0
                self.emit(format!("    and al, dl"));      // al = left && right
                self.emit(format!("    movzx {}, al", left_reg));
            }
            BinOp::Or => {
                self.emit(format!("    cmp {}, 0", left_reg));
                self.emit(format!("    setne al"));        // al = left != 0
                self.emit(format!("    cmp {}, 0", right_reg));
                self.emit(format!("    setne dl"));        // dl = right != 0
                self.emit(format!("    or al, dl"));       // al = left || right
                self.emit(format!("    movzx {}, al", left_reg));
            }
        }
    }

    fn gen_expr_num(&mut self,reg_helper: &mut RegisterHelper,num: &i64,expected_type: &Type) -> String {
        let reg = reg_helper.insert_reg(expected_type);
        self.emit(format!("    mov {}, {}", reg, num));
        reg
    }

    fn gen_expr_var(&mut self,reg_helper: &mut RegisterHelper,var_name: &String,expected_type: &Type) -> String {
        let var_data = self.lookup_var(var_name);
        let reg = reg_helper.insert_reg(expected_type);
        match var_data.var_type {
            Type::Primitive(_) => {
                let size_word = get_word(expected_type);
                self.emit(format!(
                    "    mov {}, {} [rbp - {}]",
                    reg,
                    size_word,
                    var_data.stack_pos
                ));
            }
            _ => {
                self.emit(format!(
                    "    lea {}, [rbp - {}]",
                    reg,
                    var_data.stack_pos
                ));
            }
        }
        reg
    }

    fn gen_expr_binary(&mut self,reg_helper: &mut RegisterHelper, data: (&BinOp,&Box<Expr>,&Box<Expr>),expected_type: &Type) -> String {
        let (op,left,right) = data;
        let left_reg = self.eval_expr(left, Some(reg_helper), expected_type);
        let right_reg = self.eval_expr(right, Some(reg_helper), expected_type);
        println!("reg_helper: {:?}",reg_helper.reg_stack);
        self.gen_expr_binop(op, &left_reg, &right_reg,expected_type);
        
        // we need to pop last reg because we evaluted expr
        // and results in rax but reg_stack still has both 
        reg_helper.reg_stack.pop();

        left_reg
    }

    fn gen_expr_unary(&mut self,reg_helper: &mut RegisterHelper,op: &UnaryOp,expr: &Box<Expr>,expected_type: &Type) -> String {
        match op {
            UnaryOp::Neg => {
                let reg = self.eval_expr(expr, Some(reg_helper),expected_type);
                self.emit(format!("    neg {}",reg));
                reg
            }
            UnaryOp::Not => {
                let reg = self.eval_expr(expr, Some(reg_helper),expected_type);
                self.emit(format!("    cmp {}, 0", reg));
                self.emit("    sete al".to_string());
                self.emit(format!("    movzx {}, al", reg));

                reg
            }
        }
    }

    fn gen_expr_call(&mut self, reg_helper: &mut RegisterHelper,name: &String,args: &Vec<Expr>,expected_type: &Type) -> String {
        for (index, arg) in args.iter().enumerate() {
            let arg_type = arg.get_type_of_expr(self);
            let reg = self.eval_expr(arg, Some(reg_helper), &arg_type);
            let arg_reg = arg_pos(index, &arg_type);
            self.emit(format!("    mov {}, {}", arg_reg, reg));
            reg_helper.reg_stack.pop();
        }
        
        self.emit(format!("    call {}", name));
        
        let reg = reg_helper.insert_reg(expected_type);
        let sized_rax = reg_for_size("rax", expected_type);
        
        self.emit(format!("    mov {}, {}", reg, sized_rax));

        reg
    }

    fn gen_expr_struct_init(&mut self,reg_helper: &mut RegisterHelper,fields: &Vec<(String,Expr)>, struct_name: &Option<String>,expected_type: &Type) -> String {
            let struct_name = struct_name
            .as_ref()
            .expect("StructInit missing struct name");

        let struct_data = self.structs
            .get(struct_name)
            .expect("Unknown struct")
            .clone();
        let total_size = struct_data.elements.len() * struct_data.element_size;
        let base_pos = self.alloc(total_size);

        for (field_name, field_expr) in fields {
            let field = struct_data.elements
                .get(field_name)
                .expect("Unknown field");

            let field_type = &field.ty;

            let value_reg =
                self.eval_expr(field_expr, Some(reg_helper), field_type);

            let sized_reg = reg_for_size(&value_reg, field_type);
            let size_word = get_word(field_type);

            let field_pos = base_pos - field.offset;

            self.emit(format!(
                "    mov {} [rbp - {}], {}",
                size_word,
                field_pos,
                sized_reg
            ));
        }

        let result_reg =
            reg_helper.insert_reg(&Type::Pointer(Box::new(Type::Struct(struct_name.clone()))));

        self.emit(format!(
            "    lea {}, [rbp - {}]",
            result_reg,
            base_pos
        ));

        result_reg
    }

    fn gen_expr_struct_member(&mut self, reg_helper: &mut RegisterHelper, base: &Box<Expr>, name: &String,expected_type: &Type) -> String {
        let base_reg = self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));
        let struct_name = match base.get_type_of_expr(self) {
            Type::Struct(name) => name,
            _ => self::panic!("member access on non-struct"),
        };
        let struct_data = self.structs.get(&struct_name).unwrap();
        let field = struct_data.elements.get(name).unwrap();
        self.emit(format!("    sub {}, {}", base_reg, field.offset));

        let result_reg = reg_helper.insert_reg(expected_type);
        let size_word = get_word(expected_type);

        self.emit(format!(
            "    mov {}, {} [{}]",
            result_reg,
            size_word,
            base_reg
        ));
        result_reg
    }

    fn gen_expr_deref(&mut self, reg_helper: &mut RegisterHelper, expr: &Box<Expr>,expected_type: &Type) -> String {
        let ptr_reg = self.eval_expr(expr, Some(reg_helper), &Type::Pointer(Box::new(expected_type.clone())));
        let reg = reg_helper.insert_reg(expected_type);
        let size_word = get_word(expected_type);
        self.emit(format!(
            "    mov {}, {} [{}]",
            reg,
            size_word,
            ptr_reg
        ));
        reg
    
    }

    fn gen_expr_addres_of(&mut self,reg_helper: &mut RegisterHelper, expr: &Box<Expr>) -> String {
        let ptr_type = Type::Pointer(Box::new(expr.get_type_of_expr(self)));
        match &**expr {
            Expr::Variable(name) => {
                let var = self.lookup_var(name);
                let reg = reg_helper.insert_reg(&ptr_type);

                self.emit(format!(
                    "    lea {}, [rbp - {}]",
                    reg,
                    var.stack_pos
                ));

                reg
            }

            Expr::StructMember { base, name } => {
                // evaluate base to pointer
                let base_reg =
                    self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));

                let struct_name = match base.get_type_of_expr(self) {
                    Type::Struct(name) => name,
                    _ => self::panic!("member access on non-struct"),
                };

                let struct_data = self.structs.get(&struct_name).unwrap();
                let field = struct_data.elements.get(name).unwrap();

                self.emit(format!("    sub {}, {}", base_reg, field.offset));

                base_reg
            }

            Expr::Index { base, index } => {
                let elem_type = expr.get_type_of_expr(self);
                let base_reg =
                    self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));

                let index_reg =
                    self.eval_expr(index, Some(reg_helper), &Type::Primitive(TokenType::IntType));

                let elem_size = type_size(&elem_type, &self.structs);

                self.emit(format!("    imul {}, {}, {}", index_reg, index_reg, elem_size));
                self.emit(format!("    add {}, {}", base_reg, index_reg));

                base_reg
            }

            Expr::Deref(inner) => {
                // &*ptr == ptr
                self.eval_expr(inner, Some(reg_helper), &ptr_type)
            }

            _ => self::panic!("Cannot take address of this expression"),
        }
    }


    fn gen_expr_index(&mut self, reg_helper: &mut RegisterHelper,base: &Box<Expr>,index: &Box<Expr>,expected_type: &Type) -> String {
        let base_reg = self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));
        let index_reg = self.eval_expr(index, Some(reg_helper), &Type::Primitive(TokenType::IntType));
        let elem_size = type_size(expected_type, &self.structs);

        self.emit(format!("    imul {}, {}, {}", index_reg, index_reg, elem_size));
        self.emit(format!("    add {}, {}", base_reg, index_reg));

        let result_reg = reg_helper.insert_reg(expected_type);
        let size_word = get_word(expected_type);

        self.emit(format!(
            "    mov {}, {} [{}]",
            result_reg,
            size_word,
            base_reg
        ));

        result_reg
    }


    pub fn eval_expr(&mut self, expr: &Expr, register_helper: Option<&mut RegisterHelper>, expected_type: &Type) -> String {
        let mut local_helper; // will hold the owned helper if None
        let reg_helper: &mut RegisterHelper = if let Some(rh) = register_helper {
            rh
        } else {
            local_helper = RegisterHelper { reg_stack: Vec::new() };
            &mut local_helper
        };


        match expr {
            Expr::Number(num) =>
                self.gen_expr_num(reg_helper, num, expected_type),

            Expr::Variable(var) =>
                self.gen_expr_var(reg_helper, var, expected_type),

            Expr::Binary { op, left, right } => {
                let expr_ty = expr.get_type_of_expr(self);
                self.gen_expr_binary(reg_helper, (op, left, right), &expr_ty)
            }

            Expr::Unary { op, expr: inner } => {
                self.gen_expr_unary(reg_helper, op, inner, expected_type)
            }

            Expr::Call { name, args } => {
                let ret_ty = self.functions.get(name)
                    .unwrap().return_type.clone();
                self.gen_expr_call(reg_helper, name, args, &ret_ty)
            }

            Expr::Deref(inner) => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_deref(reg_helper, inner, &ty)
            }

            Expr::AddressOf(inner) =>
                self.gen_expr_addres_of(reg_helper, inner),

            Expr::Index { base, index } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_index(reg_helper, base, index, &ty)
            }

            Expr::StructMember { base, name } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_struct_member(reg_helper, base, name, &ty)
            }

            Expr::StructInit { fields, struct_name } =>
                self.gen_expr_struct_init(reg_helper, fields, struct_name,expected_type),

            Expr::Float(_) =>
                self::panic!("floats not implemented"),
        }
    }
}
