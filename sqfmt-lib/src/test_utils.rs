use crate::config::Format;
use crate::writer::Writer;
use std::sync::Arc;

pub fn mock_format() -> Format {
    Format {
        column_limit: 20,

        indent: "    ".to_string(),
        indent_columns: 4,

        spaces_in_expr_brackets: false,

        array_spaces: false,
        array_multiline_commas: false,
        array_multiline_trailing_commas: false,
        array_singleline_trailing_commas: false,
    }
}

pub fn test_write<F: FnOnce(Writer) -> Option<Writer>>(f: F) -> String {
    f(Writer::new(Arc::new(mock_format()))).unwrap().to_string()
}

pub fn test_write_columns<F: FnOnce(Writer) -> Option<Writer>>(
    column_limit: usize,
    f: F,
) -> String {
    let format = Format {
        column_limit,
        ..mock_format()
    };
    f(Writer::new(Arc::new(format))).unwrap().to_string()
}

/// Helper to format source code with a given format configuration.
pub fn format_with(source: &str, format: Format) -> String {
    crate::format_source(source, format).unwrap()
}

/// Helper to format source code with default 4-space indent and 80-column limit.
pub fn format_test(source: &str) -> String {
    format_with(
        source,
        Format {
            column_limit: 80,
            indent: "    ".to_string(),
            indent_columns: 4,
            ..Format::default()
        },
    )
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn format_empty_function() {
        let input = "void function Foo() {}";
        let output = format_test(input);
        assert_eq!(output, "void function Foo()\n{\n}\n");
    }

    #[test]
    fn format_function_with_body() {
        let input = "void function Foo() { print(\"hello\") }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Foo()\n{\n    print( \"hello\" )\n}\n"
        );
    }

    #[test]
    fn format_if_else() {
        let input = "void function Test() { if (x) { a() } else { b() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    if ( x )\n    {\n        a()\n    }\n    else\n    {\n        b()\n    }\n}\n"
        );
    }

    #[test]
    fn format_else_if() {
        let input = "void function Test() { if (x) { a() } else if (y) { b() } else { c() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    if ( x )\n    {\n        a()\n    }\n    else if ( y )\n    {\n        b()\n    }\n    else\n    {\n        c()\n    }\n}\n"
        );
    }

    #[test]
    fn format_variable_definition() {
        let input = "void function Test() { int x = 1 + 2 }";
        let output = format_test(input);
        assert_eq!(output, "void function Test()\n{\n    int x = 1 + 2\n}\n");
    }

    #[test]
    fn format_for_loop() {
        let input = "void function Test() { for (int i = 0; i < 10; i++) print(i) }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    for ( int i = 0; i < 10; i++ )\n        print( i )\n}\n"
        );
    }

    #[test]
    fn format_array_expression() {
        let input = "void function Test() { local arr = [1, 2, 3] }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    local arr = [ 1, 2, 3 ]\n}\n"
        );
    }

    #[test]
    fn format_return_statement() {
        let input = "int function Add(int a, int b) { return a + b }";
        let output = format_test(input);
        assert_eq!(
            output,
            "int function Add( int a, int b )\n{\n    return a + b\n}\n"
        );
    }

    #[test]
    fn format_enum() {
        let input = "enum Dir { NORTH = 0, SOUTH = 1 }";
        let output = format_test(input);
        assert_eq!(output, "enum Dir\n{\n    NORTH = 0,\n    SOUTH = 1\n}\n");
    }

    #[test]
    fn format_global_enum() {
        let input = "global enum Dir { NORTH = 0, SOUTH = 1 }";
        let output = format_test(input);
        assert_eq!(
            output,
            "global enum Dir\n{\n    NORTH = 0,\n    SOUTH = 1\n}\n"
        );
    }

    #[test]
    fn format_struct() {
        let input = "struct Foo { int x, int y }";
        let output = format_test(input);
        assert_eq!(output, "struct Foo\n{\n    int x\n    int y\n}\n");
    }

    #[test]
    fn format_global_struct() {
        let input = "global struct Foo { int x, int y }";
        let output = format_test(input);
        assert_eq!(output, "global struct Foo\n{\n    int x\n    int y\n}\n");
    }

    #[test]
    fn format_inline_struct_var() {
        let input = "struct { int x, int y } file";
        let output = format_test(input);
        assert_eq!(output, "struct\n{\n    int x\n    int y\n} file\n");
    }

    #[test]
    fn format_switch() {
        let input = "void function Test() {\nswitch (x) {\ncase 0:\nprint(\"a\")\nbreak\ncase 1:\nprint(\"b\")\nbreak\n}\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    switch ( x )\n    {\n        case 0:\n            print( \"a\" )\n            break\n        case 1:\n            print( \"b\" )\n            break\n    }\n}\n"
        );
    }

    #[test]
    fn format_switch_comment_only_case() {
        // Comments in a case body with no statements should be indented at body level,
        // not at the case keyword level.
        let input =
            "void function Test() {\nswitch (x) {\ncase 0:\n// comment\ncase 1:\nbreak\n}\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    switch ( x )\n    {\n        case 0:\n            // comment\n        case 1:\n            break\n    }\n}\n"
        );
    }

    #[test]
    fn format_try_catch() {
        let input = "void function Test() { try { Danger() } catch (ex) { print(ex) } }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    try\n    {\n        Danger()\n    }\n    catch ( ex )\n    {\n        print( ex )\n    }\n}\n"
        );
    }

    #[test]
    fn format_global_function_declaration() {
        let input = "global function MyFunc";
        let output = format_test(input);
        assert_eq!(output, "global function MyFunc\n");
    }

    #[test]
    fn format_thread_statements() {
        let input = "void function Test() { thread DoThing() }";
        let output = format_test(input);
        assert_eq!(output, "void function Test()\n{\n    thread DoThing()\n}\n");
    }

    #[test]
    fn format_ternary() {
        let input = "void function Test() { local x = a ? b : c }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    local x = a ? b : c\n}\n"
        );
    }

    #[test]
    fn format_nested_calls() {
        let input = "void function Test() { Foo(Bar(Baz())) }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    Foo( Bar( Baz() ) )\n}\n"
        );
    }

    #[test]
    fn format_while_loop() {
        let input = "void function Test() { while (alive) { DoThing() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    while ( alive )\n    {\n        DoThing()\n    }\n}\n"
        );
    }

    #[test]
    fn format_do_while() {
        let input = "void function Test() { do { DoThing() } while (alive) }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    do\n    {\n        DoThing()\n    }\n    while ( alive )\n}\n"
        );
    }

    #[test]
    fn format_foreach() {
        let input = "void function Test() { foreach (val in arr) print(val) }";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Test()\n{\n    foreach ( val in arr )\n        print( val )\n}\n"
        );
    }

    #[test]
    fn format_class() {
        let input = "class Foo { x = 1 y = 2 }";
        let output = format_test(input);
        assert_eq!(output, "class Foo\n{\n    x = 1\n    y = 2\n}\n");
    }

    #[test]
    fn format_const() {
        let input = "const int MAX = 100";
        let output = format_test(input);
        assert_eq!(output, "const int MAX = 100\n");
    }

    #[test]
    fn format_nested_generic_space_before_close() {
        assert_eq!(
            format_test("table< var, table<var, var> > x"),
            "table<var, table<var, var> > x\n"
        );
    }

    #[test]
    fn format_vector_always_has_spaces() {
        assert_eq!(
            format_test("local x = <1, 2, 3>"),
            "local x = < 1, 2, 3 >\n"
        );
        assert_eq!(
            format_test("local x = <-1, -2, -3>"),
            "local x = < -1, -2, -3 >\n"
        );
    }

    #[test]
    fn format_binary_assignment_wraps_after_operator() {
        // When a binary assignment is too long, it should break after the `=`
        let output = format_with(
            "void function T() { fp.thirdPersonAnim = EVAC_EMBARK_ANIMS_3P[slot] }",
            Format {
                column_limit: 50,
                indent: "    ".to_string(),
                indent_columns: 4,
                ..Format::default()
            },
        );
        assert_eq!(
            output,
            "void function T()\n{\n    fp.thirdPersonAnim =\n        EVAC_EMBARK_ANIMS_3P[ slot ]\n}\n"
        );
    }

    // Regression tests for array formatting idempotency bugs

    #[test]
    fn array_commented_out_element_idempotent() {
        // Bug 1: commented-out array element pulled rest of array onto comment line
        let input = "struct {\n    array<string> names = [\n        // \"Arc Pylon\",\n        \"Arc Ball\"\n    ]\n} file";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert!(
            output.contains("[\n        // \"Arc Pylon\",\n        \"Arc Ball\"\n    ]"),
            "comment should stay on its own line inside array: {output}"
        );
    }

    #[test]
    fn array_trailing_comment_on_last_element_idempotent() {
        // Bug 2: last array element with trailing comment gets broken on 2nd pass
        let input = "struct {\n    float[2][3] offsets = [\n        [0.2, 0.0],\n        [0.2, 2.0], // right\n        [0.2, -2.0], // left\n    ]\n} file";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert!(
            output.contains("[ 0.2, -2.0 ] // left"),
            "last element should stay single-line with trailing comment: {output}"
        );
    }

    #[test]
    fn array_leading_comments_idempotent() {
        // Bug 3: multi-line array with leading comments gets collapsed then re-expanded
        let input = "const array<string> EVENTS = \n[\n    // these are disabled\n    // needs to re-enable them\n    \"DoomTitan\",\n    \"DoomAutoTitan\"\n]";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert!(
            output.contains("[\n    // these are disabled"),
            "comments should stay inside array on their own lines: {output}"
        );
    }

    #[test]
    fn array_last_element_no_comma_trailing_comment_idempotent() {
        // Bug 4: last element without trailing comma loses its trailing comment stability
        let input = "const array< array< string > > ANIMS = [\n    [ \"a\", \"b\", \"c\" ], // first\n    [ \"d\", \"e\", \"f\" ], // second\n    [ \"g\", \"h\", \"i\" ] // third\n]";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert!(
            output.contains("[ \"g\", \"h\", \"i\" ] // third"),
            "last element should stay single-line with trailing comment: {output}"
        );
    }

    #[test]
    fn binary_assignment_trailing_comment_stays_single_line() {
        // Trailing comments on RHS should not force multi-line split
        // Pattern A: LHS = RHS // comment
        let output = format_test("highlight.paramVecs[ 0 ] = colour // <0.8,0.4,0.2>\n");
        assert!(
            output.contains("highlight.paramVecs[ 0 ] = colour // <0.8,0.4,0.2>"),
            "assignment with trailing comment should stay on one line: {output}"
        );

        // Pattern B: LHS = literal_RHS // comment
        let output = format_test(
            "serverdetails.showchatprefix = true // GetConVarBool(\"discordlogshowteamchatprefix\")\n",
        );
        assert!(
            output.contains("serverdetails.showchatprefix = true // GetConVarBool("),
            "assignment with trailing comment should stay on one line: {output}"
        );

        // Pattern C: table slot <- property_access // comment
        let output =
            format_test("void function F() {\nparams[ \"type\" ] <- message.typeofmsg // yr\n}");
        assert!(
            output.contains("params[ \"type\" ] <- message.typeofmsg // yr"),
            "table slot assignment with property access trailing comment should stay on one line: {output}"
        );
    }

    #[test]
    fn binary_assignment_trailing_comment_idempotent() {
        let input = "highlight.paramVecs[ 0 ] = colour // <0.8,0.4,0.2>\n";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
    }

    #[test]
    fn format_preprocessor_if_blocks() {
        // #if / #else / #endif indent the enclosed code one extra level
        let input = "void function Foo() {\n#if DEV\nDoDevThing()\n#else\nDoProdThing()\n#endif\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Foo()\n{\n    #if DEV\n        DoDevThing()\n    #else\n        DoProdThing()\n    #endif\n}\n"
        );
    }

    #[test]
    fn format_preprocessor_endif_with_following_statement() {
        // #endif followed by more statements inside a block was over-indented
        let input = "void function Foo() {\n#if DEV\nDoDevThing()\n#endif\nDoProdThing()\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Foo()\n{\n    #if DEV\n        DoDevThing()\n    #endif\n    DoProdThing()\n}\n"
        );
    }

    #[test]
    fn format_preprocessor_define() {
        // #define is a non-block directive and does not change indent depth
        let input = "void function Foo() {\n#define MAX_PLAYERS 12\ndoThing()\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Foo()\n{\n    #define MAX_PLAYERS 12\n    doThing()\n}\n"
        );
    }

    #[test]
    fn format_block_with_only_comment() {
        // A block containing only a comment should have the comment indented
        let input = "void function Foo() {\n// comment\n}";
        let output = format_test(input);
        assert_eq!(output, "void function Foo()\n{\n    // comment\n}\n");
    }

    #[test]
    fn format_nested_block_with_only_comment() {
        // Comments in nested blocks should indent to the correct depth
        let input = "void function Foo() {\nif (x) {\n// comment\n}\n}";
        let output = format_test(input);
        assert_eq!(
            output,
            "void function Foo()\n{\n    if ( x )\n    {\n        // comment\n    }\n}\n"
        );
    }

    #[test]
    fn format_block_with_only_comment_idempotent() {
        let input = "void function Foo()\n{\n    // comment\n}\n";
        let output = format_test(input);
        assert_eq!(output, input);
    }

    #[test]
    fn format_lambda_capture_spaces() {
        // Capture list should have spaces inside parens: function() : ( guy )
        let input = "void function Test() { OnThreadEnd( function() : (guy) { Foo() } ) }";
        let output = format_test(input);
        assert!(
            output.contains("function() : ( guy )"),
            "capture list should have spaces inside parens: {output}"
        );
    }

    #[test]
    fn format_lambda_capture_multiple() {
        // Multiple captures should also have spaces
        let input = "void function Test() { OnThreadEnd( function() : (a, b, c) { Foo() } ) }";
        let output = format_test(input);
        assert!(
            output.contains("function() : ( a, b, c )"),
            "multi-capture list should have spaces inside parens: {output}"
        );
    }

    #[test]
    fn format_lambda_empty_capture() {
        // Empty capture list should not have spaces: function() : ()
        let input = "void function Test() { OnThreadEnd( function() : () { Foo() } ) }";
        let output = format_test(input);
        assert!(
            output.contains("function() : ()"),
            "empty capture list should not have spaces: {output}"
        );
    }

    #[test]
    fn format_spacing_test_file() {
        let input = std::fs::read_to_string("../sqfmt/test_files/spacing.gnut").unwrap();
        let output = format_with(
            &input,
            Format {
                column_limit: 120,
                indent: "\t".to_string(),
                indent_columns: 4,
                ..Format::default()
            },
        );
        assert_eq!(
            output,
            "void function example( entity player )\n{\n\tif ( IsValid( player ) )\n\t{\n\t\tif ( IsAlive( player ) )\n\t\t{\n\t\t\tif ( player.isMechanical() )\n\t\t\t{\n\t\t\t\tplayer.SetMaxHealth( 100 )\n\t\t\t}\n\t\t}\n\t}\n}\n"
        );
    }
}
