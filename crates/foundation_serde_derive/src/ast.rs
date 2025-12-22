use crate::attr::{ContainerAttr, FieldAttr, StructAttr, VariantAttr};

#[derive(Debug, Clone)]
pub struct DeriveInput {
    pub name: String,
    pub generics: crate::generics::Generics,
    pub data: Data,
    pub container_attr: ContainerAttr,
}

#[derive(Debug, Clone)]
pub enum Data {
    Struct(Struct),
    Enum(Vec<Variant>),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Struct {
    pub fields: Fields,
    pub attrs: StructAttr,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Field>),
    Unit,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Field {
    pub name: Option<String>,
    pub ty: String,
    pub attr: FieldAttr,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub fields: Fields,
    pub attr: VariantAttr,
}

#[allow(dead_code)]
impl Fields {
    pub fn len(&self) -> usize {
        match self {
            Fields::Named(fields) | Fields::Unnamed(fields) => fields.len(),
            Fields::Unit => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
