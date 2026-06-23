use super::components;

pub struct Container<'a> {
    pub(crate) components: Vec<Box<dyn components::Component + 'a>>,
}

impl<'a> std::fmt::Debug for Container<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container").finish()
    }
}

impl<'a> Default for Container<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Container<'a> {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn add<ComponentType>(&mut self, component: ComponentType)
    where
        ComponentType: components::Component + 'a,
    {
        self.components.push(Box::new(component));
    }

    pub fn extend(&mut self, other: Self) {
        self.components.extend(other.components);
    }

    pub fn render(&self) -> Vec<superconsole::Line> {
        let mut lines = Vec::new();
        for component in &self.components {
            lines.extend(component.render());
        }
        lines
    }
}
