use crate::{
    console::{ConsoleError, OutputStyle, TreeHelper, TreeNode, TreeStyle, TreeValue},
    support::ArrayKey,
};
impl OutputStyle<'_> {
    pub fn create_tree<K: Into<ArrayKey>, V: Into<TreeValue>>(
        &mut self,
        values: impl IntoIterator<Item = (K, V)>,
        root: &str,
    ) -> Result<TreeHelper<'_>, ConsoleError> {
        TreeHelper::from_reference(
            self.output.for_style_component(),
            TreeNode::new(root),
            values,
            TreeStyle::default(),
        )
    }
    pub fn tree<K: Into<ArrayKey>, V: Into<TreeValue>>(
        &mut self,
        values: impl IntoIterator<Item = (K, V)>,
        root: &str,
    ) -> Result<(), ConsoleError> {
        self.create_tree(values, root)?.render()
    }
}
