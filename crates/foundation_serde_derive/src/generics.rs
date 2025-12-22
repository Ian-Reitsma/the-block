#[derive(Debug, Clone, Default)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub where_clause: String,
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub declaration: String,
    pub name: String,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    Lifetime,
    Type,
    Const,
}

impl Generics {
    pub fn split_for_impl(&self) -> (String, String, String) {
        let impl_generics = if self.params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                self.params
                    .iter()
                    .map(|p| p.declaration.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let ty_generics = if self.params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                self.params
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        (impl_generics, ty_generics, self.where_clause.clone())
    }

    pub fn type_params(&self) -> impl Iterator<Item = &GenericParam> {
        self.params.iter().filter(|p| p.kind == ParamKind::Type)
    }

    #[allow(dead_code)]
    pub fn with_added_lifetime(&self, lifetime: &str) -> Generics {
        let mut params = self.params.clone();
        params.insert(
            0,
            GenericParam {
                declaration: lifetime.to_string(),
                name: lifetime.to_string(),
                kind: ParamKind::Lifetime,
            },
        );
        Generics {
            params,
            where_clause: self.where_clause.clone(),
        }
    }
}
