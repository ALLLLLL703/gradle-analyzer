#[derive(Clone, Default, Debug)]
pub enum GradleFileKind {
    SettingsKotlin,
    SettingsGroovy,
    BuildGroovy,
    BuildKotlin,
    #[default]
    Unknown,
}

impl GradleFileKind {
    pub fn is_settings(self) -> bool {
        matches!(self, Self::SettingsGroovy | Self::SettingsKotlin)
    }

    pub fn is_build(self) -> bool {
        matches!(self, Self::BuildGroovy | Self::BuildKotlin)
    }

    pub fn is_kotlin_dsl(self) -> bool {
        matches!(self, Self::SettingsKotlin | Self::BuildKotlin)
    }

    pub fn is_groovy_dsl(self) -> bool {
        matches!(self, Self::SettingsGroovy | Self::BuildGroovy)
    }
}
