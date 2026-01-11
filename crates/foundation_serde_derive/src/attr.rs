#[derive(Debug, Clone, Default)]
pub struct ContainerAttr {
    pub crate_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StructAttr {
    #[allow(dead_code)]
    pub rename_all: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FieldAttr {
    pub rename: Option<String>,
    pub default: FieldDefault,
}

impl Default for FieldAttr {
    fn default() -> Self {
        Self {
            rename: None,
            default: FieldDefault::None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VariantAttr {
    pub rename: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub enum FieldDefault {
    #[default]
    None,
    Default,
    Function(String),
}
