# sqformat Style Guide

This document describes every formatting rule sqformat applies. All examples show input on the left / top and formatted output on the right / bottom.

---

## Line Length

The default column limit is **160 characters**. When a construct exceeds this limit, sqformat falls back to a multi-line layout. When it fits, it stays on one line.

---

## Indentation

One **tab** per level. Tabs are assumed to be 4 columns wide.

```squirrel
// input
void function Foo() {
if (x) {
doThing()
}
}

// output
void function Foo()
{
	if ( x )
	{
		doThing()
	}
}
```

---

## Braces (Allman Style)

Opening braces appear on their **own line**. Closing braces are on their own line at the matching indentation level.

```squirrel
// input
void function Foo() { doThing() }

// output
void function Foo()
{
	doThing()
}
```

**Empty blocks** have the braces on separate lines:

```squirrel
void function Foo()
{
}
```

---

## Semicolons

Semicolons are **removed**. Do not rely on them in formatted output.

```squirrel
// input
doA();
doB();

// output
doA()
doB()
```

---

## Blank Lines

Each statement is preceded by a blank line when there is preceding content on the page. Multiple consecutive blank lines are collapsed to one.

```squirrel
// input
doA()


doB()



doC()

// output
doA()

doB()

doC()
```

---

## Spacing in Control Flow

A space appears before `(` in `if`, `while`, `for`, `foreach`, `switch`, and `catch`. Spaces also appear inside the brackets.

```squirrel
// input
if(x){}
while(running){}

// output
if ( x )
{
}

while ( running )
{
}
```

---

## Spaces Inside Brackets

Spaces are added inside parentheses in expressions and function calls, and inside square brackets in index expressions. **Empty** parentheses or brackets get no spaces.

```squirrel
// input
foo(a,b,c)
arr[0]
empty()

// output
foo( a, b, c )
arr[ 0 ]
empty()
```

When a call or index overflows the line limit, each argument/index moves to its own indented line:

```squirrel
someFunction(
	firstArgument,
	secondArgument,
	thirdArgument
)
```

---

## Spaces in Arrays

Array literals have spaces inside the brackets and after each comma.

```squirrel
// input
[1,2,3]

// output
[ 1, 2, 3 ]
```

Empty arrays get no spaces: `[]`.

Multi-line arrays indent each element:

```squirrel
[
	firstElement,
	secondElement,
	thirdElement
]
```

---

## Generic Type Brackets

**No spaces** inside generic angle brackets. However, a space is inserted before a closing `>` when it would otherwise form `>>` (which would be parsed as a right-shift operator).

```squirrel
array<int>
table<string, int>
array<array<int> >    // space before last > to avoid >>
```

---

## Binary Operators

Spaces on both sides of all binary operators: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `<=>`, `&&`, `||`, `&`, `|`, `^`, `<<`, `>>`, `>>>`, `in`, `instanceof`.

```squirrel
// input
x=a+b*c
if(x==1&&y!=2){}

// output
x = a + b * c
if ( x == 1 && y != 2 )
{
}
```

Assignment operators (`=`, `+=`, `-=`, `*=`, `/=`, `%=`, etc.) also get spaces on both sides.

---

## Prefix Operators

**Symbol prefixes** (`!`, `-`, `~`, `++`, `--`) have no space between the operator and the operand:

```squirrel
!isValid
-offset
++count
```

**Keyword prefixes** (`typeof`, `clone`, `delete`) have a space:

```squirrel
typeof value
clone original
delete slot
```

---

## Postfix Operators

No space between operand and `++` or `--`:

```squirrel
count++
index--
```

---

## Ternary Operator

Spaces around `?` and `:`:

```squirrel
// input
x=a?b:c

// output
x = a ? b : c
```

When the ternary overflows, the condition stays on the current line and `?` / `:` lead their indented continuation lines:

```squirrel
x = someReallyLongCondition
	? valueWhenTrue
	: valueWhenFalse
```

---

## Member Access

No spaces around `.`:

```squirrel
file.gameModeScoreBarUpdateRules( file.gamestate_info )
player.GetOrigin()
```

When a chain of member accesses overflows, it can break before `.`:

```squirrel
someObject
	.someMethod()
	.anotherMethod()
```

---

## Vectors

Spaces inside `< >` with commas between components:

```squirrel
< 0.0, 1.0, 0.0 >
```

---

## Single-Line Comments (`//`)

Leading and trailing whitespace is trimmed. Long comments are word-wrapped to fit the remaining columns on the current line, with each continuation line getting its own `//` prefix.

```squirrel
//    This is a very long comment that exceeds the column limit and will be wrapped
// becomes:
// This is a very long comment that
// exceeds the column limit and will
// be wrapped
```

**Trailing comments** (on the same line as code) are never wrapped.

```squirrel
doThing() // this comment stays on one line no matter how long it is
```

---

## Multi-Line Comments (`/* */`)

Single-line multi-line comments are trimmed and kept inline:

```squirrel
/* this is a comment */
```

Multi-line `/* */` comments preserve their internal line breaks, but trailing whitespace is trimmed from each line:

```squirrel
/*
 * This preserves
 * its formatting
 */
```

---

## Preprocessor Directives (`#`)

Block-forming directives (`#if`, `#ifdef`, `#ifndef`) indent the code inside them one level. `#else`/`#elseif` step back to the `#if` level and re-indent. `#endif` closes the block. The directives themselves align with the surrounding code at the same level.

```squirrel
#if CLIENT
	ClModelViewInit()
#endif

#if DEV
	DoDevThing()
#else
	DoProdThing()
#endif

#if DEV
	#if CLIENT
		DoClientDev()
	#endif
#endif
```

Non-block directives like `#define` and `#include` emit at the current indentation without changing depth.

---

## Functions

Return type (if any), then `function`, then the name, then parameters. Parameters get spaces inside the parentheses. Empty parameter lists get no spaces.

```squirrel
// input
void function DoThing(entity player,int count){print(count)}

// output
void function DoThing( entity player, int count )
{
	print( count )
}
```

---

## If / Else

`else` appears on a new line after the closing `}` of the `if` body:

```squirrel
if ( condition )
{
	doA()
}
else if ( otherCondition )
{
	doB()
}
else
{
	doC()
}
```

---

## Loops

```squirrel
for ( int i = 0; i < count; i++ )
{
	doThing( i )
}

foreach ( int index, entity ent in entities )
{
	doThing( ent )
}

while ( IsAlive( player ) )
{
	wait 0.1
}

do
{
	doThing()
}
while ( condition )
```

---

## Switch

Each `case` and `default` is at one indent level; the statements inside are at another:

```squirrel
switch ( value )
{
	case 1:
		doA()
		break

	case 2:
		doB()
		break

	default:
		doC()
		break
}
```

---

## Try / Catch

`catch` appears on a new line after the closing `}` of the `try` body:

```squirrel
try
{
	doRiskyThing()
}
catch ( exception )
{
	handleError( exception )
}
```

---

## Classes

```squirrel
class MyClass extends BaseClass
{
	int health = 100
	static int count = 0

	function constructor()
	{
		health = 100
	}
}
```

---

## Enums

Each entry on its own indented line. Source commas are preserved as-is; no trailing comma is added or removed.

```squirrel
enum eMyEnum
{
	VALUE_A,
	VALUE_B,
	VALUE_C
}
```

---

## Structs

Each field on its own indented line. Commas are removed.

```squirrel
struct MyStruct
{
	int x
	int y
	string name
}
```

---

## Tables

Spaces inside `{ }` for single-line tables. Empty tables get no spaces: `{}`. Multi-line tables indent each slot.

```squirrel
// single-line
{ key1 = val1, key2 = val2 }

// multi-line
{
	key1 = val1,
	key2 = val2
}
```

---

## Respawn-Specific Statements

```squirrel
thread MyFunction( player )
waitthread MyFunction( player )
wait 0.5
delaythread( 1.0 ) MyFunction( player )
```
