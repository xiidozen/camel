//! Port of `UtilEdgeCasesTest`: edge cases across several camel-util modules, exercised
//! through the public API.
//!
//! Not applicable in Rust (reflection): `testLoadArrayClasses` (`ObjectHelper.loadClass`) and
//! `testFindMethodsWithAnnotation` (`AnnotationHelper`).

use camel_util::backoff::{BackOff, BackOffTimerTask};
use camel_util::concurrent::{Executor, ThreadPoolExecutor, ThreadPoolRejectedPolicy};
use camel_util::file_util::{compact_path_with, strip_ext};
use camel_util::io_helper::{get_charset_name_from_content_type, to_input_stream};
use camel_util::object_helper::get_exception;
use camel_util::scanner::Scanner;
use camel_util::string_helper::{
    dash_to_camel_case_skip, normalize_whitespace, remove_starting_characters,
    replace_from_second_occurrence,
};
use camel_util::time_utils::to_milli_seconds;
use camel_util::uri_support::{create_query_string, ParamValue, Parameters};
use std::error::Error;
use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_query_string_with_empty_list() {
    let mut map = Parameters::new();
    map.insert("a".into(), "1".into());
    map.insert("b".into(), ParamValue::List(vec![]));
    map.insert("c".into(), "2".into());
    assert_eq!("a=1&c=2", create_query_string(&map));
    map.shift_remove("c");
    assert_eq!("a=1", create_query_string(&map));
}

#[test]
fn test_strip_ext_of_hidden_file_in_root() {
    assert_eq!("/.bashrc", strip_ext("/.bashrc"));
    assert_eq!("/.a", strip_ext("/.a.b"));
    assert_eq!("/a", strip_ext("/a.b"));
}

#[test]
fn test_compact_path_keeps_drive() {
    assert_eq!("C:/b", compact_path_with("C:/a/../../b", "/"));
    assert_eq!("C:/b", compact_path_with("C:/a/../b", "/"));
}

#[test]
fn test_charset_name_from_content_type() {
    assert_eq!(
        "iso-8859-1",
        get_charset_name_from_content_type("text/plain; mycharset=foo; charset=iso-8859-1")
    );
    assert_eq!(
        "UTF-8",
        get_charset_name_from_content_type("text/plain; charset=utf-8")
    );
    assert_eq!(
        "UTF-8",
        get_charset_name_from_content_type("text/plain;charset=UTF-8")
    );
    assert_eq!(
        "utf-16",
        get_charset_name_from_content_type("text/plain; charset=utf-16")
    );
}

#[test]
fn test_encoding_input_stream_with_surrogate_pair_at_buffer_end() {
    let text = format!("{}\u{1F600}end", "a".repeat(4095));
    let dir = std::env::temp_dir()
        .join("camel-util-tests")
        .join("edge-cases");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("emoji.txt");
    std::fs::write(&file, &text).unwrap();
    let mut bytes = Vec::new();
    to_input_stream(&file, Some("UTF-8"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(text, String::from_utf8(bytes).unwrap());
}

#[test]
fn test_time_pattern() {
    assert_eq!(Ok(5000), to_milli_seconds("5S"));
    assert_eq!(Ok(-5000), to_milli_seconds("-5s"));
    assert_eq!(Ok(-500), to_milli_seconds("-500ms"));
    assert_eq!(Ok(90000), to_milli_seconds("1m30s"));
    assert_eq!(Ok(5400000), to_milli_seconds("1h 30m"));
    assert_eq!(Ok(5000), to_milli_seconds("5sec"));
    assert_eq!(Ok(93784005), to_milli_seconds("1d2h3m4s5ms"));
}

#[test]
fn test_invalid_time_pattern() {
    for s in ["five", "1m30", "abc", "s"] {
        assert!(to_milli_seconds(s).is_err(), "{s}");
    }
}

#[test]
fn test_scanner_delimiter_across_buffer_boundary() {
    let input = format!("{}\r\n\r\n", "xxxxxx;".repeat(146));
    let n = Scanner::from_str(&input, Some(r"\s*;\s*")).unwrap().count();
    assert_eq!(146, n);
}

#[test]
fn test_normalize_whitespace() {
    assert_eq!("a b", normalize_whitespace("a \t b"));
    assert_eq!("a b", normalize_whitespace("a\t\tb"));
    assert_eq!("a b c", normalize_whitespace("a  b\t\tc"));
    assert_eq!("a b", normalize_whitespace("a\nb"));
}

#[test]
fn test_replace_from_second_occurrence() {
    assert_eq!(
        "a:x/C:\\dir",
        replace_from_second_occurrence("a:x/x", "x", "C:\\dir")
    );
    assert_eq!(
        "a:x/p$1",
        replace_from_second_occurrence("a:x/x", "x", "p$1")
    );
    assert_eq!(
        "x/v/v/v",
        replace_from_second_occurrence("x/x/x/x", "x", "v")
    );
}

#[test]
fn test_dash_to_camel_case_keeps_keys_after_quotes() {
    assert_eq!(
        "camel.foo['a'].bar[x-y].myOpt",
        dash_to_camel_case_skip("camel.foo['a'].bar[x-y].my-opt", true)
    );
    assert_eq!(
        "camel.args[it's-x].myOpt",
        dash_to_camel_case_skip("camel.args[it's-x].my-opt", true)
    );
}

#[test]
fn test_remove_starting_characters() {
    assert_eq!("", remove_starting_characters("///", '/'));
    assert_eq!("", remove_starting_characters("", '/'));
}

#[test]
fn test_back_off_max_delay() {
    let backoff = BackOff::builder()
        .delay_millis(2000)
        .max_delay_millis(60000)
        .multiplier(2.0)
        .build();
    let task = BackOffTimerTask::new(None, backoff, None, |_| Ok(true));
    let delays: Vec<i64> = (0..7).map(|_| task.next()).collect();
    assert_eq!(vec![4000, 8000, 16000, 32000, 60000, 60000, 60000], delays);
}

#[test]
fn test_back_off_reset() {
    let backoff = BackOff::builder().delay(Duration::from_secs(1)).build();
    let task = BackOffTimerTask::new(None, backoff, None, |_| Ok(true));
    task.next();
    task.next();
    task.reset();
    assert_eq!(1000, task.next());
    assert_eq!(1000, task.next());
}

// a -> b -> a: a cyclic cause chain built from two statics
#[derive(Debug)]
struct ErrA;
#[derive(Debug)]
struct ErrB;
static ERR_A: ErrA = ErrA;
static ERR_B: ErrB = ErrB;

impl fmt::Display for ErrA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a")
    }
}
impl fmt::Display for ErrB {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("b")
    }
}
impl Error for ErrA {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&ERR_B)
    }
}
impl Error for ErrB {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&ERR_A)
    }
}

#[test]
fn test_exception_with_cyclic_cause() {
    // must terminate, and find the cause
    assert!(get_exception::<ErrB>(&ERR_A).is_some());
    assert!(get_exception::<std::io::Error>(&ERR_A).is_none());
}

#[test]
fn test_block_policy_rejects_when_shutdown() {
    let executor = ThreadPoolExecutor::new(
        1,
        1,
        Duration::ZERO,
        Some(1),
        ThreadPoolRejectedPolicy::Block,
        "block",
    );
    executor.shutdown();
    let rejected = Arc::new(AtomicBool::new(false));
    let r = rejected.clone();
    executor.execute_rejectable(
        Box::new(|| {}),
        Box::new(move || r.store(true, Ordering::SeqCst)),
    );
    assert!(rejected.load(Ordering::SeqCst));
}
