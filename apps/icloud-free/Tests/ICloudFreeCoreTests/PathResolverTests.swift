import XCTest
@testable import ICloudFreeCore

final class PathResolverTests: XCTestCase {
    func testTildeAndICloudAliasesResolveToExpectedLocations() {
        let tilde = CloudPathResolver.resolve("~/Pictures/lightroom")
        let alias = CloudPathResolver.resolve("icloud:Pictures/lightroom")

        XCTAssertTrue(tilde.path.hasPrefix(NSHomeDirectory()))
        XCTAssertTrue(alias.path.hasSuffix("Mobile Documents/com~apple~CloudDocs/Pictures/lightroom"))
    }
}

