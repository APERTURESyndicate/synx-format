import XCTest
@testable import Synx

final class SynxValueTests: XCTestCase {

    func test_object_set_get_remove() {
        var o = SynxObject()
        o.set("a", to: .int(1))
        o.set("b", to: .string("two"))
        XCTAssertEqual(o["a"], .int(1))
        XCTAssertTrue(o.contains("b"))
        XCTAssertTrue(o.remove("a"))
        XCTAssertFalse(o.contains("a"))
    }

    func test_value_equality_order_insensitive() {
        var a = SynxObject(); a.set("x", to: .int(1)); a.set("y", to: .int(2))
        var b = SynxObject(); b.set("y", to: .int(2)); b.set("x", to: .int(1))
        XCTAssertEqual(SynxValue.object(a), .object(b))
    }

    func test_type_helpers() {
        XCTAssertTrue(SynxValue.null.isNull)
        XCTAssertEqual(SynxValue.int(5).intValue, 5)
        XCTAssertEqual(SynxValue.float(3.14).asDouble, 3.14)
        XCTAssertNil(SynxValue.string("x").intValue)
    }

    // The default string forms reflected the enum and printed the secret's text.
    func test_default_string_forms_mask_a_secret() {
        let result = Synx.parseFullActive("!active\npassword:secret hunter2\nname x\n", options: SynxOptions(env: [:]))
        var dumped = ""
        dump(result.root, to: &dumped)
        for shown in ["\(result.root)", String(describing: result.root), String(reflecting: result.root), dumped,
                      "\(SynxValue.secret("hunter2"))", String(reflecting: SynxValue.array([.secret("hunter2")]))] {
            XCTAssertFalse(shown.contains("hunter2"), shown)
        }
        XCTAssertTrue("\(result.root)".contains("[SECRET]"))
    }
}
