use crate::Ir::{
    expr::Expr,
    r#gen::{FuncData, StructData},
    stmt::{EnumData, Type},
};

pub trait TypeContext {
    fn ensure_monomorphized(&self, ty: &Type) -> Type;
    fn monomorphize_enum(&self, def: &EnumData, type_args: &Vec<Type>) -> Type;
    fn monomorphize_struct(&self, def: &StructData, type_args: &Vec<Type>) -> Type;
    fn field_alignment(&self, ty: &Type) -> usize;
    fn resolve_call(
        &self,
        name: &String,
        args: &Vec<Expr>,
        generics: &Vec<Type>,
    ) -> Option<(FuncData, usize)>;
}
