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
    use indoc::indoc;

    #[test]
    fn format_empty_function() {
        let input = "void function Foo() {}";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                }
            "}
        );
    }

    #[test]
    fn format_function_with_body() {
        let input = "void function Foo() { print(\"hello\") }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {r#"
                void function Foo()
                {
                    print( "hello" )
                }
            "#}
        );
    }

    #[test]
    fn format_if_else() {
        let input = "void function Test() { if (x) { a() } else { b() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    if ( x )
                    {
                        a()
                    }
                    else
                    {
                        b()
                    }
                }
            "}
        );
    }

    #[test]
    fn format_else_if() {
        let input = "void function Test() { if (x) { a() } else if (y) { b() } else { c() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    if ( x )
                    {
                        a()
                    }
                    else if ( y )
                    {
                        b()
                    }
                    else
                    {
                        c()
                    }
                }
            "}
        );
    }

    #[test]
    fn format_variable_definition() {
        let input = "void function Test() { int x = 1 + 2 }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    int x = 1 + 2
                }
            "}
        );
    }

    #[test]
    fn format_for_loop() {
        let input = "void function Test() { for (int i = 0; i < 10; i++) print(i) }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    for ( int i = 0; i < 10; i++ )
                        print( i )
                }
            "}
        );
    }

    #[test]
    fn format_array_expression() {
        let input = "void function Test() { local arr = [1, 2, 3] }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    local arr = [ 1, 2, 3 ]
                }
            "}
        );
    }

    #[test]
    fn format_return_statement() {
        let input = "int function Add(int a, int b) { return a + b }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                int function Add( int a, int b )
                {
                    return a + b
                }
            "}
        );
    }

    #[test]
    fn format_enum() {
        let input = "enum Dir { NORTH = 0, SOUTH = 1 }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                enum Dir
                {
                    NORTH = 0,
                    SOUTH = 1
                }
            "}
        );
    }

    #[test]
    fn format_global_enum() {
        let input = "global enum Dir { NORTH = 0, SOUTH = 1 }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                global enum Dir
                {
                    NORTH = 0,
                    SOUTH = 1
                }
            "}
        );
    }

    #[test]
    fn format_struct() {
        let input = "struct Foo { int x, int y }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                struct Foo
                {
                    int x
                    int y
                }
            "}
        );
    }

    #[test]
    fn format_global_struct() {
        let input = "global struct Foo { int x, int y }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                global struct Foo
                {
                    int x
                    int y
                }
            "}
        );
    }

    #[test]
    fn format_inline_struct_var() {
        let input = "struct { int x, int y } file";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                struct
                {
                    int x
                    int y
                } file
            "}
        );
    }

    #[test]
    fn format_switch() {
        let input = indoc! {"
            void function Test() {
            switch (x) {
            case 0:
            print(\"a\")
            break
            case 1:
            print(\"b\")
            break
            }
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {r#"
                void function Test()
                {
                    switch ( x )
                    {
                        case 0:
                            print( "a" )
                            break

                        case 1:
                            print( "b" )
                            break
                    }
                }
            "#}
        );
    }

    #[test]
    fn format_switch_comment_only_case() {
        // Comments in a case body with no statements should be indented at body level,
        // not at the case keyword level.
        let input = indoc! {"
            void function Test() {
            switch (x) {
            case 0:
            // comment
            case 1:
            break
            }
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    switch ( x )
                    {
                        case 0:
                            // comment

                        case 1:
                            break
                    }
                }
            "}
        );
    }

    #[test]
    fn format_switch_fallthrough_cases() {
        // Consecutive cases with empty bodies (fallthrough) should not be separated
        // by blank lines. Cases with bodies should still have blank lines between them.
        let input = indoc! {r#"
            void function Test() {
            switch (x) {
            case "a":
            case "b":
            case "c":
            print("abc")
            break
            case "d":
            print("d")
            break
            default:
            break
            }
            }"#};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {r#"
                void function Test()
                {
                    switch ( x )
                    {
                        case "a":
                        case "b":
                        case "c":
                            print( "abc" )
                            break

                        case "d":
                            print( "d" )
                            break

                        default:
                            break
                    }
                }
            "#}
        );
    }

    #[test]
    fn format_try_catch() {
        let input = "void function Test() { try { Danger() } catch (ex) { print(ex) } }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    try
                    {
                        Danger()
                    }
                    catch ( ex )
                    {
                        print( ex )
                    }
                }
            "}
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
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    thread DoThing()
                }
            "}
        );
    }

    #[test]
    fn format_ternary() {
        let input = "void function Test() { local x = a ? b : c }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    local x = a ? b : c
                }
            "}
        );
    }

    #[test]
    fn format_nested_calls() {
        let input = "void function Test() { Foo(Bar(Baz())) }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    Foo( Bar( Baz() ) )
                }
            "}
        );
    }

    #[test]
    fn format_while_loop() {
        let input = "void function Test() { while (alive) { DoThing() } }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    while ( alive )
                    {
                        DoThing()
                    }
                }
            "}
        );
    }

    #[test]
    fn format_do_while() {
        let input = "void function Test() { do { DoThing() } while (alive) }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    do
                    {
                        DoThing()
                    }
                    while ( alive )
                }
            "}
        );
    }

    #[test]
    fn format_foreach() {
        let input = "void function Test() { foreach (val in arr) print(val) }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Test()
                {
                    foreach ( val in arr )
                        print( val )
                }
            "}
        );
    }

    #[test]
    fn format_class() {
        let input = "class Foo { x = 1 y = 2 }";
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                class Foo
                {
                    x = 1
                    y = 2
                }
            "}
        );
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
    fn format_vector_trailing_comment_stays_single_line() {
        assert_eq!(
            format_test("local x = <\n    1,\n    2,\n    3\n> // trailing comment"),
            "local x = < 1, 2, 3 > // trailing comment\n"
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
            indoc! {"
                void function T()
                {
                    fp.thirdPersonAnim =
                        EVAC_EMBARK_ANIMS_3P[slot]
                }
            "}
        );
    }

    // Regression tests for array formatting idempotency bugs

    #[test]
    fn array_commented_out_element_idempotent() {
        // Bug 1: commented-out array element pulled rest of array onto comment line
        let input = indoc! {r#"
            struct {
                array<string> names = [
                    // "Arc Pylon",
                    "Arc Ball"
                ]
            } file"#};
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert_eq!(
            output,
            indoc! {r#"
                struct
                {
                    array<string> names = [
                        // "Arc Pylon",
                        "Arc Ball"
                    ]
                } file
            "#}
        );
    }

    #[test]
    fn array_trailing_comment_on_last_element_idempotent() {
        // Bug 2: last array element with trailing comment gets broken on 2nd pass
        let input = indoc! {"
            struct {
                float[2][3] offsets = [
                    [0.2, 0.0],
                    [0.2, 2.0], // right
                    [0.2, -2.0], // left
                ]
            } file"};
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert_eq!(
            output,
            indoc! {"
                struct
                {
                    float[ 2 ][ 3 ] offsets = [
                        [ 0.2, 0.0 ],
                        [ 0.2, 2.0 ], // right
                        [ 0.2, -2.0 ] // left
                    ]
                } file
            "}
        );
    }

    #[test]
    fn array_leading_comments_idempotent() {
        // Bug 3: multi-line array with leading comments gets collapsed then re-expanded
        let input = indoc! {r#"
            const array<string> EVENTS =
            [
                // these are disabled
                // needs to re-enable them
                "DoomTitan",
                "DoomAutoTitan"
            ]"#};
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert_eq!(
            output,
            indoc! {r#"
                const array<string> EVENTS = [
                    // these are disabled
                    // needs to re-enable them
                    "DoomTitan",
                    "DoomAutoTitan"
                ]
            "#}
        );
    }

    #[test]
    fn array_last_element_no_comma_trailing_comment_idempotent() {
        // Bug 4: last element without trailing comma loses its trailing comment stability
        let input = indoc! {r#"
            const array< array< string > > ANIMS = [
                [ "a", "b", "c" ], // first
                [ "d", "e", "f" ], // second
                [ "g", "h", "i" ] // third
            ]"#};
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
        assert_eq!(
            output,
            indoc! {r#"
                const array<array<string> > ANIMS = [
                    [ "a", "b", "c" ], // first
                    [ "d", "e", "f" ], // second
                    [ "g", "h", "i" ] // third
                ]
            "#}
        );
    }

    #[test]
    fn binary_assignment_trailing_comment_stays_single_line() {
        // Trailing comments on RHS should not force multi-line split
        // Pattern A: LHS = RHS // comment
        assert_eq!(
            format_test("highlight.paramVecs[ 0 ] = colour // <0.8,0.4,0.2>\n"),
            "highlight.paramVecs[0] = colour // <0.8,0.4,0.2>\n"
        );

        // Pattern B: LHS = literal_RHS // comment
        assert_eq!(
            format_test(
                "serverdetails.showchatprefix = true // GetConVarBool(\"discordlogshowteamchatprefix\")\n"
            ),
            "serverdetails.showchatprefix = true // GetConVarBool(\"discordlogshowteamchatprefix\")\n"
        );

        // Pattern C: table slot <- property_access // comment
        assert_eq!(
            format_test(indoc! {r#"
                void function F() {
                params[ "type" ] <- message.typeofmsg // yr
                }"#}),
            indoc! {r#"
                void function F()
                {
                    params["type"] <- message.typeofmsg // yr
                }
            "#}
        );
    }

    #[test]
    fn binary_assignment_trailing_comment_idempotent() {
        let input = "highlight.paramVecs[0] = colour // <0.8,0.4,0.2>\n";
        let output = format_test(input);
        let output2 = format_test(&output);
        assert_eq!(output, output2, "not idempotent");
    }

    #[test]
    fn format_preprocessor_if_blocks() {
        // #if / #else / #endif indent the enclosed code one extra level
        let input = indoc! {"
            void function Foo() {
            #if DEV
            DoDevThing()
            #else
            DoProdThing()
            #endif
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                    #if DEV
                        DoDevThing()
                    #else
                        DoProdThing()
                    #endif
                }
            "}
        );
    }

    #[test]
    fn format_preprocessor_endif_with_following_statement() {
        // #endif followed by more statements inside a block was over-indented
        let input = indoc! {"
            void function Foo() {
            #if DEV
            DoDevThing()
            #endif
            DoProdThing()
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                    #if DEV
                        DoDevThing()
                    #endif
                    DoProdThing()
                }
            "}
        );
    }

    #[test]
    fn format_preprocessor_define() {
        // #define is a non-block directive and does not change indent depth
        let input = indoc! {"
            void function Foo() {
            #define MAX_PLAYERS 12
            doThing()
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                    #define MAX_PLAYERS 12
                    doThing()
                }
            "}
        );
    }

    #[test]
    fn format_block_with_only_comment() {
        // A block containing only a comment should have the comment indented
        let input = indoc! {"
            void function Foo() {
            // comment
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                    // comment
                }
            "}
        );
    }

    #[test]
    fn format_nested_block_with_only_comment() {
        // Comments in nested blocks should indent to the correct depth
        let input = indoc! {"
            void function Foo() {
            if (x) {
            // comment
            }
            }"};
        let output = format_test(input);
        assert_eq!(
            output,
            indoc! {"
                void function Foo()
                {
                    if ( x )
                    {
                        // comment
                    }
                }
            "}
        );
    }

    #[test]
    fn format_block_with_only_comment_idempotent() {
        let input = indoc! {"
            void function Foo()
            {
                // comment
            }
        "};
        let output = format_test(input);
        assert_eq!(output, input);
    }

    #[test]
    fn format_lambda_capture_spaces() {
        let input = "void function Test() { OnThreadEnd( function() : (guy) { Foo() } ) }";
        assert_eq!(
            format_test(input),
            indoc! {"
                void function Test()
                {
                    OnThreadEnd(
                        function() : ( guy )
                        {
                            Foo()
                        }
                    )
                }
            "}
        );
    }

    #[test]
    fn format_lambda_capture_multiple() {
        let input = "void function Test() { OnThreadEnd( function() : (a, b, c) { Foo() } ) }";
        assert_eq!(
            format_test(input),
            indoc! {"
                void function Test()
                {
                    OnThreadEnd(
                        function() : ( a, b, c )
                        {
                            Foo()
                        }
                    )
                }
            "}
        );
    }

    #[test]
    fn format_lambda_empty_capture() {
        // Empty capture list should not have spaces: function() : ()
        let input = "void function Test() { OnThreadEnd( function() : () { Foo() } ) }";
        assert_eq!(
            format_test(input),
            indoc! {"
                void function Test()
                {
                    OnThreadEnd(
                        function() : ()
                        {
                            Foo()
                        }
                    )
                }
            "}
        );
    }

    #[test]
    fn format_nested_if_compact_input() {
        let input = indoc! {"
            void function example(entity player) {
            if (IsValid(player)) {
            if (IsAlive(player)) {
            if (player.isMechanical()) {
            player.SetMaxHealth(100)
            }
            }
            }
            }"};
        let output = format_with(
            input,
            Format {
                column_limit: 120,
                indent: "\t".to_string(),
                indent_columns: 4,
                ..Format::default()
            },
        );
        assert_eq!(
            output,
            indoc! {"
                void function example( entity player )
                {
                \tif ( IsValid( player ) )
                \t{
                \t\tif ( IsAlive( player ) )
                \t\t{
                \t\t\tif ( player.isMechanical() )
                \t\t\t{
                \t\t\t\tplayer.SetMaxHealth( 100 )
                \t\t\t}
                \t\t}
                \t}
                }
            "}
        );
    }

    #[test]
    fn format_nested_if_spaced_input() {
        let input = indoc! {"
            void function example(entity player) {
                if (IsValid(player)) {
                    if (IsAlive(player)) {
                        if (player.isMechanical()) {
                            player.SetMaxHealth(100)
                        }
                    }
                }
            }"};
        let output = format_with(
            input,
            Format {
                column_limit: 120,
                indent: "\t".to_string(),
                indent_columns: 4,
                ..Format::default()
            },
        );
        assert_eq!(
            output,
            indoc! {"
                void function example( entity player )
                {
                \tif ( IsValid( player ) )
                \t{
                \t\tif ( IsAlive( player ) )
                \t\t{
                \t\t\tif ( player.isMechanical() )
                \t\t\t{
                \t\t\t\tplayer.SetMaxHealth( 100 )
                \t\t\t}
                \t\t}
                \t}
                }
            "}
        );
    }

    #[test]
    fn format_if_without_braces() {
        let input = indoc! {"
            void function example(entity player) {
                if (IsValid(player))
                    player.SetMaxHealth(100)
            }"};
        let output = format_with(
            input,
            Format {
                column_limit: 120,
                indent: "\t".to_string(),
                indent_columns: 4,
                ..Format::default()
            },
        );
        assert_eq!(
            output,
            indoc! {"
                void function example( entity player )
                {
                \tif ( IsValid( player ) )
                \t\tplayer.SetMaxHealth( 100 )
                }
            "}
        );
    }
}
