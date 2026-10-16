//! Code mode: run a short JavaScript program that calls tools through the router, so a
//! multi-step chain is one turn and only its final result enters Claude's context.
//!
//! QuickJS is embedded with no filesystem, network, process or module access. The only
//! way out is `call()`, which goes through the same deny list as `call_tool`.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use rquickjs::{CatchResultExt, Context, Function, Runtime};
use serde_json::{Value, json};

pub struct Limits {
    pub timeout: Duration,
    pub max_calls: usize,
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            max_calls: 50,
            memory_bytes: 64 << 20,
        }
    }
}

/// `call(tool, args)` for the script: the tool's result as JSON, or an error message.
pub type ToolCall = Box<dyn Fn(&str, Value) -> Result<Value, String>>;

const PRELUDE: &str = r#"
const __log = [];
const __fmt = (x) => typeof x === "string" ? x : JSON.stringify(x);
const console = { log: (...a) => { __log.push(a.map(__fmt).join(" ")); } };
console.error = console.log; console.info = console.log; console.warn = console.log;
function call(tool, args) {
  const r = JSON.parse(__call(String(tool), JSON.stringify(args ?? {})));
  if ("error" in r) throw new Error(r.error);
  return r.value;
}
"#;

/// Run `code` as the body of a function; its `return` value and console output come back as JSON.
pub fn run(code: &str, call: ToolCall, limits: &Limits) -> Result<Value, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(limits.memory_bytes);
    runtime.set_max_stack_size(1 << 20);
    let deadline = Instant::now() + limits.timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;

    let calls = Rc::new(Cell::new(0usize));
    let max_calls = limits.max_calls;
    let counter = calls.clone();
    let host = move |tool: String, args: String| -> String {
        counter.set(counter.get() + 1);
        let outcome = if counter.get() > max_calls {
            Err(format!("call limit of {max_calls} reached"))
        } else if Instant::now() > deadline {
            Err("script timed out".to_string())
        } else {
            serde_json::from_str(&args)
                .map_err(|e| format!("bad arguments: {e}"))
                .and_then(|args| call(&tool, args))
        };
        match outcome {
            Ok(value) => json!({ "value": value }),
            Err(error) => json!({ "error": error }),
        }
        .to_string()
    };

    context.with(|ctx| {
        let function = Function::new(ctx.clone(), host).map_err(|e| e.to_string())?;
        ctx.globals().set("__call", function).map_err(|e| e.to_string())?;
        let source = format!(
            "{PRELUDE}\nconst __result = (function() {{\n{code}\n}})();\nJSON.stringify({{ result: __result ?? null, log: __log }})"
        );
        let output: String = ctx.eval(source).catch(&ctx).map_err(|e| {
            if Instant::now() > deadline {
                format!("script timed out after {:?}", limits.timeout)
            } else {
                format!("script error: {e}")
            }
        })?;
        let mut value: Value = serde_json::from_str(&output).map_err(|e| e.to_string())?;
        if value["log"].as_array().is_some_and(Vec::is_empty) {
            value.as_object_mut().map(|o| o.remove("log"));
        }
        value["calls"] = json!(calls.get());
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_tools() -> ToolCall {
        Box::new(|tool, args| match tool {
            "math.add" => Ok(json!(
                args["a"].as_i64().unwrap() + args["b"].as_i64().unwrap()
            )),
            "repo.list" => {
                Ok(json!([{"n": 1, "bug": true}, {"n": 2, "bug": false}, {"n": 3, "bug": true}]))
            }
            _ => Err(format!("unknown tool {tool}")),
        })
    }

    #[test]
    fn chains_calls_and_returns_only_the_result() {
        let code = r#"
            const issues = call("repo.list", {});
            const bugs = issues.filter(i => i.bug).map(i => i.n);
            console.log("found", bugs.length);
            return { bugs, sum: call("math.add", { a: bugs[0], b: bugs[1] }) };
        "#;
        let out = run(code, fake_tools(), &Limits::default()).unwrap();
        assert_eq!(out["result"], json!({"bugs": [1, 3], "sum": 4}));
        assert_eq!(out["log"], json!(["found 2"]));
        assert_eq!(out["calls"], 2);
    }

    #[test]
    fn tool_errors_are_catchable_exceptions() {
        let code = r#"try { call("nope.x", {}); } catch (e) { return e.message; }"#;
        let out = run(code, fake_tools(), &Limits::default()).unwrap();
        assert_eq!(out["result"], "unknown tool nope.x");
    }

    #[test]
    fn enforces_limits_and_has_no_host_access() {
        let limits = Limits {
            timeout: Duration::from_millis(200),
            ..Limits::default()
        };
        let err = run("while (true) {}", fake_tools(), &limits).unwrap_err();
        assert!(err.contains("timed out"), "{err}");

        let limits = Limits {
            max_calls: 2,
            ..Limits::default()
        };
        let err = run(
            r#"for (;;) call("math.add", {a: 1, b: 1});"#,
            fake_tools(),
            &limits,
        )
        .unwrap_err();
        assert!(err.contains("call limit"), "{err}");

        let out = run(
            "return [typeof require, typeof fetch, typeof std, typeof os];",
            fake_tools(),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            out["result"],
            json!(["undefined", "undefined", "undefined", "undefined"])
        );

        assert!(
            run("this is not js", fake_tools(), &Limits::default())
                .unwrap_err()
                .contains("script error")
        );
    }
}
