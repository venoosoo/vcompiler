use crate::Ir::expr::{BinOp, Expr, UnaryOp};
use crate::sem_analysis::coerce_numeric;

use super::*;
use super::{get_word, reg_for_size};

pub struct RegisterHelper {
    reg_stack: Vec<String>,
}

// TODO needs rewrite to support both Gen and Analyzer
// needs traits
impl Expr {
    /// Returns the Type of this expression
    pub fn get_type_of_expr(&self, gen_helper: &Gen) -> Type {
        match self {
            Expr::Number(_) => Type::Primitive(TokenType::LongType),
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
                coerce_numeric(&lty, &rty)
            }
            Expr::StructInit {
                struct_name_ty,
                fields: _,
            } => {
                if let Some(_struct_data) = gen_helper.structs.get(struct_name_ty) {
                    Type::Struct(struct_name_ty.clone())
                } else {
                    self::panic!("Struct {} not found in get_type_of_expr", struct_name_ty);
                }
            }

            Expr::Deref(ptr_expr) => match ptr_expr.get_type_of_expr(gen_helper) {
                Type::Pointer(inner) => *inner,
                _ => self::panic!("Cannot dereference a non-pointer"),
            },
            Expr::AddressOf(var_expr) => {
                let ty = var_expr.get_type_of_expr(gen_helper);
                Type::Pointer(Box::new(ty))
            }
            Expr::Index { base, index } => {
                let base_ty = base.get_type_of_expr(gen_helper);
                let idx_ty = index.get_type_of_expr(gen_helper);
                if idx_ty != Type::Primitive(TokenType::LongType) {
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
                        let struct_data = gen_helper
                            .structs
                            .get(struct_name)
                            .expect(&format!("Struct {} not found", struct_name));
                        let field = struct_data.elements.get(name).expect(&format!(
                            "Field {} not found in struct {}",
                            name, struct_name
                        ));
                        field.ty.clone()
                    }
                    _ => self::panic!("Cannot access member of non-struct type"),
                }
            }
            Expr::Call { name, args: _ } => {
                let func_data = gen_helper
                    .functions
                    .get(name)
                    .expect(&format!("Function {} not found", name));
                func_data.return_type.clone()
            }
            Expr::ArrayInit { elements } => {
                if elements.len() > 0 {
                    return elements[0].get_type_of_expr(gen_helper);
                } else {
                    Type::Unknown
                }
            }
        }
    }
}

impl RegisterHelper {
    pub fn insert_reg(&mut self, ty: &Type) -> Option<String> {
        if self.reg_stack.len() == 0 {
            let reg_res: Option<String> = reg_for_size("rax", ty);
            if let Some(reg) = reg_res {
                self.reg_stack.push(reg.clone());
                return Some(reg);
            } else {
                return None;
            }
        } else if self.reg_stack.len() == 1 {
            let reg_res = reg_for_size("rbx", ty);
            if let Some(reg) = reg_res {
                self.reg_stack.push(reg.clone());
                return Some(reg);
            } else {
                return None;
            }
        } else {
            None
        }
    }
    pub fn get_reg(&mut self) -> String {
        self.reg_stack
            .pop()
            .expect("RegisterHelper get_reg trying to get register but its empty")
    }
}

impl Gen {
    fn gen_expr_binop(
        &mut self,
        op: &BinOp,
        left_reg: &str,
        right_reg: &str,
        expected_type: &Type,
    ) {
        match op {
            BinOp::Add => {
                self.emit_main(format!("    add {}, {}", left_reg, right_reg));
            }
            BinOp::Sub => {
                self.emit_main(format!("    sub {}, {}", left_reg, right_reg));
            }
            BinOp::Mul => {
                self.emit_main(format!("    imul {}, {}", left_reg, right_reg));
            }
            BinOp::Div | BinOp::Mod => {
                if self.type_size(expected_type) == 8 {
                    self.emit_main("    cqo".to_string()); // 64-bit
                } else {
                    self.emit_main("    cdq".to_string()); // 32-bit
                }

                self.emit_main(format!("    idiv {}", right_reg));

                if let BinOp::Mod = op {
                    self.emit_main(format!(
                        "    mov {}, {}",
                        left_reg,
                        reg_for_size("rdx", expected_type).unwrap()
                    ));
                }
            }
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.emit_main(format!("    cmp {}, {}", left_reg, right_reg));
                let set_instr = match op {
                    BinOp::Eq => "sete",
                    BinOp::Neq => "setne",
                    BinOp::Lt => "setl",
                    BinOp::Lte => "setle",
                    BinOp::Gt => "setg",
                    BinOp::Gte => "setge",
                    _ => unreachable!(),
                };
                self.emit_main(format!("    {} al", set_instr));
                self.emit_main(format!("    movzx {}, al", left_reg));
            }
            BinOp::And => {
                self.emit_main(format!("    cmp {}, 0", left_reg));
                self.emit_main(format!("    setne al")); // al = left != 0
                self.emit_main(format!("    cmp {}, 0", right_reg));
                self.emit_main(format!("    setne dl")); // dl = right != 0
                self.emit_main(format!("    and al, dl")); // al = left && right
                self.emit_main(format!("    movzx {}, al", left_reg));
            }
            BinOp::Or => {
                self.emit_main(format!("    cmp {}, 0", left_reg));
                self.emit_main(format!("    setne al")); // al = left != 0
                self.emit_main(format!("    cmp {}, 0", right_reg));
                self.emit_main(format!("    setne dl")); // dl = right != 0
                self.emit_main(format!("    or al, dl")); // al = left || right
                self.emit_main(format!("    movzx {}, al", left_reg));
            }
        }
    }

    fn gen_expr_num(
        &mut self,
        reg_helper: &mut RegisterHelper,
        num: &i64,
        expected_type: &Type,
    ) -> String {
        let reg = reg_helper.insert_reg(expected_type);
        if let Some(asm_reg) = reg {
            self.emit_main(format!("    mov {}, {}", asm_reg, num));
            asm_reg
        } else {
            return num.to_string();
        }
    }

    fn gen_expr_var(
        &mut self,
        reg_helper: &mut RegisterHelper,
        var_name: &String,
        expected_type: &Type,
    ) -> String {
        let var_data = self.lookup_var(var_name);
        let some_reg = reg_helper.insert_reg(expected_type);
        if let Some(reg) = some_reg {
            match var_data.var_type {
                Type::Primitive(_) => {
                    let actual_size = self.type_size(&var_data.var_type);
                    let expected_size = self.type_size(expected_type);
                    if var_data.global_flag {
                        let var_reg = format!("[rel {}]",var_name);
                        self.emit_main(format!("    mov {}, {}",reg,var_reg));
                        return reg;
                        
                    }
                    
                    if expected_size > actual_size {
                        // sign-extend smaller type into larger register
                        let src_word = get_word(&var_data.var_type);
                        self.emit_main(format!(
                            "    movsx {}, {} [rbp - {}]",
                            reg, src_word, var_data.stack_pos
                        ));
                    } else {
                        let size_word = get_word(expected_type);
                        self.emit_main(format!(
                            "    mov {}, {} [rbp - {}]",
                            reg, size_word, var_data.stack_pos
                        ));
                    }
                }
                Type::Pointer(_) => {
                    if var_data.global_flag {
                        self.emit(format!("    mov {}, [rel {}]",reg,var_name));
                    } else {
                        self.emit_main(format!("    mov {}, [rbp - {}]", reg, var_data.stack_pos));
                    }
                }
                _ => {
                    if var_data.global_flag {
                        self.emit_main(format!("    lea {}, [rel {}]", reg, var_name));
                    } else {
                        self.emit_main(format!("    lea {}, [rbp - {}]", reg, var_data.stack_pos));
                    }
                }
            }
            reg
        } else {
            return format!("[rbp - {}]", var_data.stack_pos);
        }
    }

    fn gen_expr_binary(
        &mut self,
        reg_helper: &mut RegisterHelper,
        data: (&BinOp, &Box<Expr>, &Box<Expr>),
        expected_type: &Type,
    ) -> String {
        let (op, left, right) = data;
        let left_reg = self.eval_expr(left, Some(reg_helper), expected_type);
        let right_reg = self.eval_expr(right, Some(reg_helper), expected_type);
        self.gen_expr_binop(op, &left_reg, &right_reg, expected_type);

        // we need to pop last reg because we evaluted expr
        // and results in rax but reg_stack still has both
        reg_helper.get_reg();

        left_reg
    }

    fn gen_expr_unary(
        &mut self,
        reg_helper: &mut RegisterHelper,
        op: &UnaryOp,
        expr: &Box<Expr>,
        expected_type: &Type,
    ) -> String {
        match op {
            UnaryOp::Neg => {
                let reg = self.eval_expr(expr, Some(reg_helper), expected_type);
                self.emit_main(format!("    neg {}", reg));
                reg
            }
            UnaryOp::Not => {
                let reg = self.eval_expr(expr, Some(reg_helper), expected_type);
                self.emit_main(format!("    cmp {}, 0", reg));
                self.emit_main("    sete al".to_string());
                self.emit_main(format!("    movzx {}, al", reg));

                reg
            }
        }
    }

    fn gen_expr_call(
        &mut self,
        reg_helper: &mut RegisterHelper,
        name: &String,
        args: &Vec<Expr>,
        expected_type: &Type,
    ) -> String {
        let func_data = self.functions.get(name).unwrap().clone();
        for (index, arg) in args.iter().enumerate() {
            let func_stmt = &func_data.args[index];
            let arg_type = func_stmt.get_type_gen(self).unwrap();

            let reg = self.eval_expr(arg, Some(reg_helper), &arg_type);
            let arg_reg = arg_pos(index, &arg_type);
            self.emit_main(format!("    mov {}, {}", arg_reg, reg));
            reg_helper.get_reg();
        }

        self.emit_main(format!("    call {}", name));
        let reg_asm = reg_helper.insert_reg(expected_type);
        if let Some(reg) = reg_asm {
            let sized_rax = reg_for_size("rax", expected_type).unwrap();

            self.emit_main(format!("    mov {}, {}", reg, sized_rax));

            reg
        } else {
            return "rax".to_string();
        }
    }

    fn gen_expr_struct_init(
        &mut self,
        reg_helper: &mut RegisterHelper,
        fields: &Vec<(String, Expr)>,
        struct_name: &String,
        expected_type: &Type,
    ) -> String {
        let struct_data = self
            .structs
            .get(struct_name)
            .expect("Unknown struct")
            .clone();
        let total_size = struct_data.elements.len() * struct_data.element_size;
        let base_pos = self.stack_pos;

        for (field_name, field_expr) in fields {
            let field = struct_data.elements.get(field_name).expect("Unknown field");

            let field_type = &field.ty;
            self.eval_expr(field_expr, None, expected_type);
            let sized_reg = reg_for_size("rax", field_type).unwrap();
            let size_word = get_word(field_type);

            let field_pos = base_pos - field.offset;

            self.emit_main(format!(
                "    mov {} [rbp - {}], {}",
                size_word, field_pos, sized_reg
            ));
        }

        let result_reg =
            reg_helper.insert_reg(&Type::Pointer(Box::new(Type::Struct(struct_name.clone()))));
        if let Some(result_reg) = result_reg {
            result_reg
        } else {
            println!("wtf");
            "rax".to_string()
        }
    }

    fn gen_expr_struct_member(
        &mut self,
        reg_helper: &mut RegisterHelper,
        base: &Box<Expr>,
        name: &String,
        expected_type: &Type,
    ) -> String {
        let base_reg = self.eval_expr(base, None, &base.get_type_of_expr(self));
        let struct_name = match base.get_type_of_expr(self) {
            Type::Struct(name) => name,
            _ => self::panic!("member access on non-struct"),
        };
        let struct_data = self.structs.get(&struct_name).unwrap();
        let field = struct_data.elements.get(name).unwrap();
        self.emit_main(format!("    add {}, {}", base_reg, field.offset));

        let result_reg = reg_helper.insert_reg(expected_type);
        if let Some(result_reg) = result_reg {
            let size_word = get_word(expected_type);

            self.emit_main(format!(
                "    mov {}, {} [{}]",
                result_reg, size_word, base_reg
            ));
            result_reg
        } else {
            self::panic!("wtf");
        }
    }

    fn gen_expr_deref(
        &mut self,
        reg_helper: &mut RegisterHelper,
        expr: &Box<Expr>,
        expected_type: &Type,
    ) -> String {
        let ptr_reg = self.eval_expr(
            expr,
            Some(reg_helper),
            &Type::Pointer(Box::new(expected_type.clone())),
        );
        let asm_reg = reg_helper.insert_reg(expected_type);
        if let Some(reg) = asm_reg {
            let size_word = get_word(expected_type);
            self.emit_main(format!("    mov {}, {} [{}]", reg, size_word, ptr_reg));
            reg
        } else {
            self::panic!("wtf");
        }
    }

    fn gen_expr_addres_of(&mut self, reg_helper: &mut RegisterHelper, expr: &Box<Expr>) -> String {
        let ptr_type = Type::Pointer(Box::new(expr.get_type_of_expr(self)));
        match &**expr {
            Expr::Variable(name) => {
                let var = self.lookup_var(name);
                let asm_reg = reg_helper.insert_reg(&ptr_type);
                if let Some(reg) = asm_reg {
                    self.emit_main(format!("    lea {}, [rbp - {}]", reg, var.stack_pos));

                    reg
                } else {
                    self::panic!("wtf");
                }
            }

            Expr::StructMember { base, name } => {
                // evaluate base to pointer
                let base_reg = self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));

                let struct_name = match base.get_type_of_expr(self) {
                    Type::Struct(name) => name,
                    _ => self::panic!("member access on non-struct"),
                };

                let struct_data = self.structs.get(&struct_name).unwrap();
                let field = struct_data.elements.get(name).unwrap();

                self.emit_main(format!("    sub {}, {}", base_reg, field.offset));

                base_reg
            }

            Expr::Index { base, index } => {
                let elem_type = expr.get_type_of_expr(self);
                let base_reg = self.eval_expr(base, Some(reg_helper), &base.get_type_of_expr(self));

                let index_reg = self.eval_expr(
                    index,
                    Some(reg_helper),
                    &Type::Primitive(TokenType::LongType),
                );

                let elem_size = self.type_size(&elem_type);

                self.emit_main(format!(
                    "    imul {}, {}, {}",
                    index_reg, index_reg, elem_size
                ));
                self.emit_main(format!("    add {}, {}", base_reg, index_reg));

                base_reg
            }

            Expr::Deref(inner) => {
                // &*ptr == ptr
                self.eval_expr(inner, Some(reg_helper), &ptr_type)
            }

            _ => self::panic!("Cannot take address of this expression"),
        }
    }

    fn gen_expr_index(
        &mut self,
        reg_helper: &mut RegisterHelper,
        base: &Box<Expr>,
        index: &Box<Expr>,
        expected_type: &Type,
    ) -> String {
        let arr_ty = &base.get_type_of_expr(self);
        let base_reg = self.eval_expr(base, Some(reg_helper), arr_ty);
        let index_reg = self.eval_expr(
            index,
            Some(reg_helper),
            &Type::Primitive(TokenType::LongType),
        );
        match arr_ty {
            Type::Array(ty, size) => {
                self.emit_main(format!("    cmp {}, {}", index_reg, size));
                self.emit_main(format!("    jge __bounds_fail__"));
                self.emit_main(format!("    cmp {}, 0", index_reg));
                self.emit_main(format!("    jl __bounds_fail__"));
            }
            _ => {}
        }
        let elem_size = self.type_size(expected_type);

        self.emit_main(format!(
            "    imul {}, {}, {}",
            index_reg, index_reg, elem_size
        ));
        self.emit_main(format!("    add {}, {}", base_reg, index_reg));
        reg_helper.get_reg();
        let reg = reg_helper.insert_reg(expected_type);
        if let Some(result_reg) = reg {
            let size_word = get_word(expected_type);

            self.emit_main(format!(
                "    mov {}, {} [{}]",
                result_reg, size_word, base_reg
            ));

            result_reg
        } else {
            self::panic!("wtf");
        }
    }

    fn gen_array_init(
        &mut self,
        reg_helper: &mut RegisterHelper,
        elements: &Vec<Expr>,
        expected_type: &Type,
    ) -> String {
        let elem_type = match expected_type {
            Type::Array(elem_ty, _) => *elem_ty.clone(),
            _ => self::panic!("gen_array_init called with non-array type"),
        };
        let elem_size = self.type_size(&elem_type);
        let base_pos = self.stack_pos;

        for (i, elem) in elements.iter().enumerate() {
            let sized_reg = reg_for_size("rax", &elem_type).unwrap();
            let size_word = get_word(&elem_type);
            let offset = base_pos - (i * elem_size);

            self.emit_main(format!(
                "    mov {} [rbp - {}], {}",
                size_word, offset, sized_reg
            ));
            reg_helper.reg_stack.pop();
        }
        let reg = reg_helper.insert_reg(&Type::Pointer(Box::new(elem_type.clone())));
        if let Some(result_reg) = reg {
            self.emit_main(format!("    lea {}, [rbp - {}]", result_reg, base_pos));

            result_reg
        } else {
            self::panic!("wtf");
        }
    }

    pub fn eval_expr(
        &mut self,
        expr: &Expr,
        register_helper: Option<&mut RegisterHelper>,
        expected_type: &Type,
    ) -> String {
        let mut local_helper; // will hold the owned helper if None
        let reg_helper: &mut RegisterHelper = if let Some(rh) = register_helper {
            rh
        } else {
            local_helper = RegisterHelper {
                reg_stack: Vec::new(),
            };
            &mut local_helper
        };
        match expr {
            Expr::ArrayInit { elements } => {
                self.gen_array_init(reg_helper, elements, expected_type)
            }
            Expr::Number(num) => self.gen_expr_num(reg_helper, num, expected_type),

            Expr::Variable(var) => self.gen_expr_var(reg_helper, var, expected_type),

            Expr::Binary { op, left, right } => {
                let expr_ty = expr.get_type_of_expr(self);
                self.gen_expr_binary(reg_helper, (op, left, right), &expr_ty)
            }

            Expr::Unary { op, expr: inner } => {
                self.gen_expr_unary(reg_helper, op, inner, expected_type)
            }

            Expr::Call { name, args } => {
                let ret_ty = self.functions.get(name).unwrap().return_type.clone();
                self.gen_expr_call(reg_helper, name, args, &ret_ty)
            }

            Expr::Deref(inner) => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_deref(reg_helper, inner, &ty)
            }

            Expr::AddressOf(inner) => self.gen_expr_addres_of(reg_helper, inner),

            Expr::Index { base, index } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_index(reg_helper, base, index, &ty)
            }

            Expr::StructMember { base, name } => {
                let ty = expr.get_type_of_expr(self);
                self.gen_expr_struct_member(reg_helper, base, name, &ty)
            }

            Expr::StructInit {
                fields,
                struct_name_ty,
            } => self.gen_expr_struct_init(reg_helper, fields, struct_name_ty, expected_type),

            Expr::Float(_) => self::panic!("floats not implemented"),
        }
    }
}
