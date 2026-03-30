pub struct Format {
    pub column_limit: usize,

    pub indent: String,
    pub indent_columns: usize,

    pub spaces_in_expr_brackets: bool,

    pub array_spaces: bool,
    pub array_multiline_commas: bool,
    pub array_multiline_trailing_commas: bool,
    pub array_singleline_trailing_commas: bool,
}

impl Default for Format {
    fn default() -> Self {
        Format {
            column_limit: 120,
            indent: "\t".to_string(),
            indent_columns: 4,
            spaces_in_expr_brackets: true,
            array_spaces: true,
            array_multiline_commas: true, // Setting this to false can break arrays, needs fix
            array_multiline_trailing_commas: false,
            array_singleline_trailing_commas: false,
        }
    }
}
