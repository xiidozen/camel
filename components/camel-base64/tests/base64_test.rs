//! Ports of the camel-base64 tests: `Base64DataFormatDefaultsTest`,
//! `Base64DataFormatDSLDefaultsTest`, `Base64DataFormatLineLengthTest`,
//! `Base64DataFormatLineEndingsTest`, `SimpleBase64Test`, `Base64PropertiesFunctionTest`,
//! `Base64PropertiesFunctionDecodeTest` and `Base64PropertiesFunctionOptionalTest`.

use camel_base64::Base64DataFormat;
use camel_core::api::spi::data_format::SharedDataFormat;
use camel_core::prelude::*;
use std::sync::Arc;

/// The decoded data of the Java tests (`Base64DataFormatTestBase.DECODED`).
const DECODED: &[u8] = &[
    34, 180, 86, 132, 214, 77, 140, 92, 80, 233, 101, 201, 16, 139, 30, 133, 185, 197, 118, 234,
    237, 129, 167, 0, 171, 235, 122, 102, 29, 238, 98, 92, 156, 148, 93, 199, 3, 31, 125, 230, 102,
    200, 201, 212, 37, 202, 99, 60, 87, 124, 128, 249, 134, 245, 167, 114, 35, 236, 4, 181, 85, 6,
    148, 52, 112, 37, 140, 218, 71, 82, 170, 214, 71, 161, 38, 1, 38, 85, 252, 68, 47, 71, 145,
    164, 241, 112, 239, 70, 186, 237, 146, 128, 230, 250, 55, 89, 253, 114, 159, 122, 203, 118, 56,
    140, 225, 139, 22, 69, 214, 103, 42, 202, 45, 79, 29, 234, 127, 172, 89, 213, 61, 201, 78, 245,
    99, 150, 180, 32, 145, 135, 63, 148, 218, 202, 1, 204, 208, 109, 189, 55, 58, 199, 90, 115,
    214, 3, 153, 114, 131, 132, 20, 161, 210, 127, 105, 38, 146, 173, 127, 238, 17, 197, 151, 144,
    155, 246, 118, 210, 6, 12, 169, 148, 92, 236, 34, 117, 65, 124, 41, 17, 197, 112, 5, 139, 28,
    27, 11, 112, 126, 44, 31, 30, 215, 156, 210, 50, 250, 6, 234, 137, 50, 14, 206, 194, 241, 4,
    161, 33, 144, 151, 24, 121, 108, 185, 72, 135, 247, 168, 179, 81, 183, 193, 242, 170, 83, 64,
    175, 52, 60, 240, 34, 179, 22, 33, 23, 248, 27, 235, 191, 48, 188, 238, 94, 255, 77, 192, 152,
    89, 152, 108, 26, 179, 194, 131, 80, 232, 250, 25, 40, 82, 60, 107, 131, 212, 172, 186, 2, 46,
    181, 39, 41, 8, 48, 57, 123, 158, 111, 92, 188, 119, 134, 236, 84, 106, 215, 31, 148, 20, 22,
    0, 4, 223, 38, 68, 159, 102, 80, 181, 91, 250, 109, 153, 230, 222, 91, 193, 123, 242, 203, 68,
    62, 49, 33, 77, 143, 114, 110, 94, 248, 50, 84, 102, 17, 220, 151, 255, 12, 106, 42, 3, 88, 88,
    24, 60, 183, 237, 12, 171, 196, 83, 2, 193, 220, 129, 86, 45, 34, 140, 40, 90, 30, 94, 125,
    167, 147, 12, 108, 8, 122, 243, 133, 168, 225, 110, 66, 165, 215, 225, 253, 35, 177, 164, 119,
    95, 67, 200, 10, 15, 34, 184, 150, 56, 148, 156, 162, 15, 166, 112, 253, 34, 168, 111, 98, 50,
    166, 5, 146, 141, 167, 174, 29, 171, 175, 232, 220, 200, 161, 65, 134, 180, 84, 184, 220, 55,
    91, 207, 161, 248, 83, 176, 100, 50, 220, 6, 107, 147, 191, 105, 68, 58, 245, 29, 104, 248, 3,
    251, 87, 186, 125, 122, 110, 198, 24, 162, 135, 140, 31, 255, 57, 90, 145, 229, 90, 211, 125,
    83, 245, 205, 63, 28, 221, 54, 207, 71, 0, 132, 180, 245, 190, 209, 0, 211, 48, 231, 232, 236,
    243, 133, 143, 255, 50, 29, 247, 217, 153, 130, 251, 26, 232, 249, 239, 104, 217, 174, 190,
    118, 124, 26, 152, 59, 75, 185, 24, 3, 13, 223, 132, 114, 133, 16, 165, 241, 11, 94, 134, 215,
    221, 148, 151, 236, 228, 48, 213, 149, 241, 90, 34, 75, 181, 35, 156, 231, 34, 209, 145, 214,
    241, 116, 178, 191, 11, 167, 228, 113, 142, 28, 169, 207, 209, 192, 17, 152, 181, 115, 123,
    103, 81, 13, 27, 251, 63, 46, 122, 217, 157, 21, 63, 18, 179, 22, 70, 214, 209, 193, 232, 64,
    255, 70, 182, 41, 38, 205, 56, 204, 10, 240, 26, 115, 14, 187, 151, 185, 242, 215, 101, 193,
    154, 12, 164, 84, 122, 75, 80, 125, 213, 248, 94, 24, 88, 17, 43, 117, 118, 183, 176, 149, 2,
    213, 104, 69, 171, 100, 20, 124, 36, 231, 121, 153, 201, 46, 148, 248, 112, 30, 157, 144, 211,
    70, 50, 161, 39, 4, 143, 164, 108, 255, 219, 34, 202, 254, 61, 252, 84, 154, 68, 38, 231, 127,
    57, 119, 9, 25, 205, 188, 35, 131, 213, 203, 197, 224, 233, 222, 97, 181, 73, 8, 195, 152, 227,
    219, 156, 22, 99, 127, 152, 16, 178, 196, 179, 173, 39, 134, 115, 240, 65, 181, 20, 47, 53, 65,
    56, 55, 73, 204, 62, 21, 209, 67, 176, 40, 43, 236, 58, 149, 174, 99, 206, 46, 41, 42, 171,
    172, 9, 116, 176, 99, 200, 117, 238, 50, 37, 79, 244, 90, 191, 104, 12, 111, 92, 72, 35, 70,
    229, 103, 55, 109, 48, 159, 107, 84, 57, 119, 154, 55, 133, 234, 206, 36, 58, 232, 205, 182,
    244, 123, 24, 208, 21, 249, 174, 34, 116, 211, 37, 192, 172, 60, 93, 248, 143, 102, 20, 58,
    112, 253, 64, 231, 24, 16, 4, 216, 255, 255, 213, 245, 53, 158, 205, 64, 235, 52, 58, 133, 59,
    251, 107, 23, 101, 195, 127, 197, 156, 224, 29, 202, 97, 10, 168, 64, 253, 8, 247, 219, 136,
    59, 201, 202, 7, 0, 115, 27, 246, 127, 35, 145, 29, 15, 147, 138, 154, 204, 27, 23, 36, 2, 89,
    203, 103, 106, 70, 151, 232, 14, 205, 187, 89, 204, 152, 30, 115, 33, 73, 28, 22, 225, 182, 75,
    155, 24, 62, 51, 205, 8, 110, 36, 156, 60, 202, 154, 169, 165, 44, 106, 14, 207, 18, 1, 109,
    159, 174, 62, 54, 81, 63, 106, 68, 199, 130, 4, 101, 203, 107, 92, 50, 33, 43, 120, 24, 142,
    162, 58, 119, 240, 76, 36, 73, 253, 33, 59, 9, 90, 196, 126, 103, 102, 68, 233, 73, 164, 2, 71,
    125, 73, 80, 32, 154, 181, 105, 109, 230, 76, 78, 115, 78, 96, 50, 131, 42, 113, 69, 64, 194,
    152, 15, 158, 99, 220, 29, 10, 39, 5, 167, 166, 41, 181, 208, 132, 43, 115, 246, 237, 12, 217,
    177, 32, 18, 0, 28, 157, 230, 60, 71, 50, 34, 1, 145, 220, 6, 206, 61, 121, 211, 92, 89, 238,
    17, 75, 36, 53, 195, 77,
];

const ENCODED_DEFAULTS: &str = "IrRWhNZNjFxQ6WXJEIsehbnFdurtgacAq+t6Zh3uYlyclF3HAx995mbIydQlymM8V3yA+Yb1p3Ij\r\n7AS1VQaUNHAljNpHUqrWR6EmASZV/EQvR5Gk8XDvRrrtkoDm+jdZ/XKfest2OIzhixZF1mcqyi1P\r\nHep/rFnVPclO9WOWtCCRhz+U2soBzNBtvTc6x1pz1gOZcoOEFKHSf2kmkq1/7hHFl5Cb9nbSBgyp\r\nlFzsInVBfCkRxXAFixwbC3B+LB8e15zSMvoG6okyDs7C8QShIZCXGHlsuUiH96izUbfB8qpTQK80\r\nPPAisxYhF/gb678wvO5e/03AmFmYbBqzwoNQ6PoZKFI8a4PUrLoCLrUnKQgwOXueb1y8d4bsVGrX\r\nH5QUFgAE3yZEn2ZQtVv6bZnm3lvBe/LLRD4xIU2Pcm5e+DJUZhHcl/8MaioDWFgYPLftDKvEUwLB\r\n3IFWLSKMKFoeXn2nkwxsCHrzhajhbkKl1+H9I7Gkd19DyAoPIriWOJScog+mcP0iqG9iMqYFko2n\r\nrh2rr+jcyKFBhrRUuNw3W8+h+FOwZDLcBmuTv2lEOvUdaPgD+1e6fXpuxhiih4wf/zlakeVa031T\r\n9c0/HN02z0cAhLT1vtEA0zDn6OzzhY//Mh332ZmC+xro+e9o2a6+dnwamDtLuRgDDd+EcoUQpfEL\r\nXobX3ZSX7OQw1ZXxWiJLtSOc5yLRkdbxdLK/C6fkcY4cqc/RwBGYtXN7Z1ENG/s/LnrZnRU/ErMW\r\nRtbRwehA/0a2KSbNOMwK8BpzDruXufLXZcGaDKRUektQfdX4XhhYESt1drewlQLVaEWrZBR8JOd5\r\nmckulPhwHp2Q00YyoScEj6Rs/9siyv49/FSaRCbnfzl3CRnNvCOD1cvF4OneYbVJCMOY49ucFmN/\r\nmBCyxLOtJ4Zz8EG1FC81QTg3Scw+FdFDsCgr7DqVrmPOLikqq6wJdLBjyHXuMiVP9Fq/aAxvXEgj\r\nRuVnN20wn2tUOXeaN4XqziQ66M229HsY0BX5riJ00yXArDxd+I9mFDpw/UDnGBAE2P//1fU1ns1A\r\n6zQ6hTv7axdlw3/FnOAdymEKqED9CPfbiDvJygcAcxv2fyORHQ+TiprMGxckAlnLZ2pGl+gOzbtZ\r\nzJgecyFJHBbhtkubGD4zzQhuJJw8ypqppSxqDs8SAW2frj42UT9qRMeCBGXLa1wyISt4GI6iOnfw\r\nTCRJ/SE7CVrEfmdmROlJpAJHfUlQIJq1aW3mTE5zTmAygypxRUDCmA+eY9wdCicFp6YptdCEK3P2\r\n7QzZsSASAByd5jxHMiIBkdwGzj1501xZ7hFLJDXDTQ==\r\n";
const ENCODED_LINE_LENGTH_64: &str = "IrRWhNZNjFxQ6WXJEIsehbnFdurtgacAq+t6Zh3uYlyclF3HAx995mbIydQlymM8\r\nV3yA+Yb1p3Ij7AS1VQaUNHAljNpHUqrWR6EmASZV/EQvR5Gk8XDvRrrtkoDm+jdZ\r\n/XKfest2OIzhixZF1mcqyi1PHep/rFnVPclO9WOWtCCRhz+U2soBzNBtvTc6x1pz\r\n1gOZcoOEFKHSf2kmkq1/7hHFl5Cb9nbSBgyplFzsInVBfCkRxXAFixwbC3B+LB8e\r\n15zSMvoG6okyDs7C8QShIZCXGHlsuUiH96izUbfB8qpTQK80PPAisxYhF/gb678w\r\nvO5e/03AmFmYbBqzwoNQ6PoZKFI8a4PUrLoCLrUnKQgwOXueb1y8d4bsVGrXH5QU\r\nFgAE3yZEn2ZQtVv6bZnm3lvBe/LLRD4xIU2Pcm5e+DJUZhHcl/8MaioDWFgYPLft\r\nDKvEUwLB3IFWLSKMKFoeXn2nkwxsCHrzhajhbkKl1+H9I7Gkd19DyAoPIriWOJSc\r\nog+mcP0iqG9iMqYFko2nrh2rr+jcyKFBhrRUuNw3W8+h+FOwZDLcBmuTv2lEOvUd\r\naPgD+1e6fXpuxhiih4wf/zlakeVa031T9c0/HN02z0cAhLT1vtEA0zDn6OzzhY//\r\nMh332ZmC+xro+e9o2a6+dnwamDtLuRgDDd+EcoUQpfELXobX3ZSX7OQw1ZXxWiJL\r\ntSOc5yLRkdbxdLK/C6fkcY4cqc/RwBGYtXN7Z1ENG/s/LnrZnRU/ErMWRtbRwehA\r\n/0a2KSbNOMwK8BpzDruXufLXZcGaDKRUektQfdX4XhhYESt1drewlQLVaEWrZBR8\r\nJOd5mckulPhwHp2Q00YyoScEj6Rs/9siyv49/FSaRCbnfzl3CRnNvCOD1cvF4One\r\nYbVJCMOY49ucFmN/mBCyxLOtJ4Zz8EG1FC81QTg3Scw+FdFDsCgr7DqVrmPOLikq\r\nq6wJdLBjyHXuMiVP9Fq/aAxvXEgjRuVnN20wn2tUOXeaN4XqziQ66M229HsY0BX5\r\nriJ00yXArDxd+I9mFDpw/UDnGBAE2P//1fU1ns1A6zQ6hTv7axdlw3/FnOAdymEK\r\nqED9CPfbiDvJygcAcxv2fyORHQ+TiprMGxckAlnLZ2pGl+gOzbtZzJgecyFJHBbh\r\ntkubGD4zzQhuJJw8ypqppSxqDs8SAW2frj42UT9qRMeCBGXLa1wyISt4GI6iOnfw\r\nTCRJ/SE7CVrEfmdmROlJpAJHfUlQIJq1aW3mTE5zTmAygypxRUDCmA+eY9wdCicF\r\np6YptdCEK3P27QzZsSASAByd5jxHMiIBkdwGzj1501xZ7hFLJDXDTQ==\r\n";
const ENCODED_LINE_ENDINGS: &str = "IrRWhNZNjFxQ6WXJEIsehbnFdurtgacAq+t6Zh3uYlyclF3HAx995mbIydQlymM8V3yA+Yb1p3Ij\n7AS1VQaUNHAljNpHUqrWR6EmASZV/EQvR5Gk8XDvRrrtkoDm+jdZ/XKfest2OIzhixZF1mcqyi1P\nHep/rFnVPclO9WOWtCCRhz+U2soBzNBtvTc6x1pz1gOZcoOEFKHSf2kmkq1/7hHFl5Cb9nbSBgyp\nlFzsInVBfCkRxXAFixwbC3B+LB8e15zSMvoG6okyDs7C8QShIZCXGHlsuUiH96izUbfB8qpTQK80\nPPAisxYhF/gb678wvO5e/03AmFmYbBqzwoNQ6PoZKFI8a4PUrLoCLrUnKQgwOXueb1y8d4bsVGrX\nH5QUFgAE3yZEn2ZQtVv6bZnm3lvBe/LLRD4xIU2Pcm5e+DJUZhHcl/8MaioDWFgYPLftDKvEUwLB\n3IFWLSKMKFoeXn2nkwxsCHrzhajhbkKl1+H9I7Gkd19DyAoPIriWOJScog+mcP0iqG9iMqYFko2n\nrh2rr+jcyKFBhrRUuNw3W8+h+FOwZDLcBmuTv2lEOvUdaPgD+1e6fXpuxhiih4wf/zlakeVa031T\n9c0/HN02z0cAhLT1vtEA0zDn6OzzhY//Mh332ZmC+xro+e9o2a6+dnwamDtLuRgDDd+EcoUQpfEL\nXobX3ZSX7OQw1ZXxWiJLtSOc5yLRkdbxdLK/C6fkcY4cqc/RwBGYtXN7Z1ENG/s/LnrZnRU/ErMW\nRtbRwehA/0a2KSbNOMwK8BpzDruXufLXZcGaDKRUektQfdX4XhhYESt1drewlQLVaEWrZBR8JOd5\nmckulPhwHp2Q00YyoScEj6Rs/9siyv49/FSaRCbnfzl3CRnNvCOD1cvF4OneYbVJCMOY49ucFmN/\nmBCyxLOtJ4Zz8EG1FC81QTg3Scw+FdFDsCgr7DqVrmPOLikqq6wJdLBjyHXuMiVP9Fq/aAxvXEgj\nRuVnN20wn2tUOXeaN4XqziQ66M229HsY0BX5riJ00yXArDxd+I9mFDpw/UDnGBAE2P//1fU1ns1A\n6zQ6hTv7axdlw3/FnOAdymEKqED9CPfbiDvJygcAcxv2fyORHQ+TiprMGxckAlnLZ2pGl+gOzbtZ\nzJgecyFJHBbhtkubGD4zzQhuJJw8ypqppSxqDs8SAW2frj42UT9qRMeCBGXLa1wyISt4GI6iOnfw\nTCRJ/SE7CVrEfmdmROlJpAJHfUlQIJq1aW3mTE5zTmAygypxRUDCmA+eY9wdCicFp6YptdCEK3P2\n7QzZsSASAByd5jxHMiIBkdwGzj1501xZ7hFLJDXDTQ==\n";

/// A started context with the base64 extensions and the routes.
async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_base64::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.clone(),
        other => other.to_string().into_bytes(),
    }
}

/// `Base64DataFormatTestBase`: encodes and decodes with the data format.
async fn run(
    df: impl Into<camel_core::model::processor::DataFormatDefinition> + Clone + Send + Sync + 'static,
    encoded: &str,
) {
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:startEncode")
                .marshal(df.clone())
                .to("mock:result"),
        );
        r.add(
            from("direct:startDecode")
                .unmarshal(df.clone())
                .to("mock:result"),
        );
    })
    .await;
    let template = c.create_producer_template();
    let out = template
        .request_body("direct:startEncode", Value::Bytes(DECODED.to_vec()))
        .await
        .unwrap();
    assert_eq!(encoded, String::from_utf8(bytes(&out)).unwrap());
    let out = template
        .request_body(
            "direct:startDecode",
            Value::Bytes(encoded.as_bytes().to_vec()),
        )
        .await
        .unwrap();
    assert_eq!(DECODED, &bytes(&out)[..]);
}

fn df(d: Base64DataFormat) -> SharedDataFormat {
    Arc::new(d)
}

#[tokio::test]
async fn test_defaults() {
    run(df(Base64DataFormat::new()), ENCODED_DEFAULTS).await;
}

/// The data format by name (Java `.marshal().base64()`).
#[tokio::test]
async fn test_dsl_defaults() {
    run("base64", ENCODED_DEFAULTS).await;
}

#[tokio::test]
async fn test_line_length() {
    run(
        df(Base64DataFormat::new().with_line_length(64)),
        ENCODED_LINE_LENGTH_64,
    )
    .await;
    run(
        camel_core::model::builder::data_format("base64").option("lineLength", 64),
        ENCODED_LINE_LENGTH_64,
    )
    .await;
}

#[tokio::test]
async fn test_line_endings() {
    run(
        df(Base64DataFormat::new().with_line_separator(b"\n")),
        ENCODED_LINE_ENDINGS,
    )
    .await;
}

/// `SimpleBase64Test`.
#[tokio::test]
async fn test_simple_base64() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:encode").set_body(simple("${base64Encode()}")));
        r.add(from("direct:encodeBody").set_body(simple("${base64Encode(${body})}")));
        r.add(from("direct:decode").set_body(simple("${base64Decode()}")));
        r.add(from("direct:decodeBody").set_body(simple("${base64Decode(${body})}")));
    })
    .await;
    let t = c.create_producer_template();
    for uri in ["direct:encode", "direct:encodeBody"] {
        assert_eq!(
            "Q2FtZWw=\r\n",
            t.request_body(uri, "Camel").await.unwrap().to_string()
        );
    }
    for uri in ["direct:decode", "direct:decodeBody"] {
        assert_eq!(
            b"Camel".to_vec(),
            bytes(&t.request_body(uri, "Q2FtZWw=").await.unwrap())
        );
    }
}

/// `Base64PropertiesFunctionTest`, `Base64PropertiesFunctionDecodeTest` and
/// `Base64PropertiesFunctionOptionalTest`.
#[tokio::test]
async fn test_properties_function() {
    for (exp, expected) in [
        ("${body} {{base64:fooKey}}", "Hello Camel"),
        ("${body} {{base64:decode:Q2FtZWw==}}", "Hello Camel"),
        ("${body} {{base64:?missingKey}}", "Hello "),
    ] {
        let context = camel_core::default_context();
        camel_base64::register(&context);
        context
            .properties()
            .add_initial_property("fooKey", "Q2FtZWw==");
        context
            .add_routes(move |r: &mut Routes| {
                r.add(from("direct:start").set_body(simple(exp)).to("mock:result"));
            })
            .await
            .unwrap();
        context.start().await.unwrap();
        let out = context
            .create_producer_template()
            .request_body("direct:start", "Hello")
            .await
            .unwrap();
        assert_eq!(expected, out.to_string(), "{exp}");
    }
}
