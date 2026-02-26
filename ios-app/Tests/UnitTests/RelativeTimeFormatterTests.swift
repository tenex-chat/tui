import XCTest
@testable import TenexMVP

final class RelativeTimeFormatterTests: XCTestCase {
    func testJustNowForPastUnderOneMinute() {
        let now = Date(timeIntervalSince1970: 10_000)
        let date = now.addingTimeInterval(-59)

        XCTAssertEqual(RelativeTime.string(date: date, now: now, style: .localizedAbbreviated), "just now")
        XCTAssertEqual(RelativeTime.string(date: date, now: now, style: .compact), "just now")
    }

    func testNotJustNowAtOneMinuteBoundary() {
        let now = Date(timeIntervalSince1970: 10_000)
        let date = now.addingTimeInterval(-60)

        XCTAssertNotEqual(RelativeTime.string(date: date, now: now, style: .localizedAbbreviated), "just now")
        XCTAssertEqual(RelativeTime.string(date: date, now: now, style: .compact), "1m ago")
    }

    func testFutureTimestampDoesNotUseJustNow() {
        let now = Date(timeIntervalSince1970: 10_000)
        let future = now.addingTimeInterval(30)

        XCTAssertNotEqual(RelativeTime.string(date: future, now: now, style: .localizedAbbreviated), "just now")
        XCTAssertEqual(RelativeTime.string(date: future, now: now, style: .compact), "in <1m")
    }

    func testCompactStyleBoundaries() {
        let now = Date(timeIntervalSince1970: 10_000)

        XCTAssertEqual(RelativeTime.string(date: now.addingTimeInterval(-60), now: now, style: .compact), "1m ago")
        XCTAssertEqual(RelativeTime.string(date: now.addingTimeInterval(-3_600), now: now, style: .compact), "1h ago")
        XCTAssertEqual(RelativeTime.string(date: now.addingTimeInterval(-86_400), now: now, style: .compact), "1d ago")
    }

    func testAdaptiveScheduleUsesSecondTicksInJustNowWindow() throws {
        let startDate = Date(timeIntervalSince1970: 10_001)
        let referenceDate = startDate.addingTimeInterval(-30)
        var entries = RelativeTimeSchedule(referenceDate: referenceDate).entries(from: startDate, mode: .normal).makeIterator()

        let first = try XCTUnwrap(entries.next())
        let second = try XCTUnwrap(entries.next())
        let third = try XCTUnwrap(entries.next())

        XCTAssertEqual(first.timeIntervalSince1970, 10_002, accuracy: 0.0001)
        XCTAssertEqual(second.timeIntervalSince(first), 1, accuracy: 0.0001)
        XCTAssertEqual(third.timeIntervalSince(second), 1, accuracy: 0.0001)
    }

    func testAdaptiveScheduleUsesMinuteTicksOutsideJustNowWindow() throws {
        let startDate = Date(timeIntervalSince1970: 10_001)
        let referenceDate = startDate.addingTimeInterval(-3_600)
        var entries = RelativeTimeSchedule(referenceDate: referenceDate).entries(from: startDate, mode: .normal).makeIterator()

        let first = try XCTUnwrap(entries.next())
        let second = try XCTUnwrap(entries.next())

        XCTAssertEqual(first.timeIntervalSince1970, 10_020, accuracy: 0.0001)
        XCTAssertEqual(second.timeIntervalSince(first), 60, accuracy: 0.0001)
    }

    func testDateFromAgeSeconds() {
        let referenceNow = Date(timeIntervalSince1970: 10_000)
        let date = RelativeTime.date(referenceNow: referenceNow, ageSeconds: 42)

        XCTAssertEqual(date.timeIntervalSince1970, 9_958, accuracy: 0.0001)
    }
}
