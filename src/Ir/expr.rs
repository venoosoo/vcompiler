
#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64),
    Float(f64),
    Variable(String),

    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    Call {
        name: String,
        args: Vec<Expr>,
    },

    StructInit {
        struct_name: Option<String>,
        fields: Vec<(String, Expr)>
    },

    StructMember {
        base: Box<Expr>,
        name: String,
    },

    Deref(Box<Expr>),
    AddressOf(Box<Expr>),

    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}


#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}


#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
}