import XCTest
@testable import TenexMVP

final class TodoStateTests: XCTestCase {

    // MARK: - Helpers

    private func makeTodo(id: String = "todo-0", title: String = "Task", status: TodoStatus = .pending) -> TodoItem {
        TodoItem(id: id, title: title, description: nil, status: status, skipReason: nil)
    }

    // MARK: - TodoState.completedCount

    func testCompletedCountIsZeroForEmptyItems() {
        let state = TodoState(items: [])
        XCTAssertEqual(state.completedCount, 0)
    }

    func testCompletedCountCountsDoneItems() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .pending),
            makeTodo(id: "2", status: .done),
        ])
        XCTAssertEqual(state.completedCount, 2)
    }

    func testCompletedCountCountsCompletedAsDone() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .completed),
            makeTodo(id: "1", status: .done),
        ])
        XCTAssertEqual(state.completedCount, 2)
    }

    func testCompletedCountDoesNotCountPendingInProgressOrSkipped() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .pending),
            makeTodo(id: "1", status: .inProgress),
            makeTodo(id: "2", status: .skipped),
        ])
        XCTAssertEqual(state.completedCount, 0)
    }

    // MARK: - TodoState.inProgressItem

    func testInProgressItemReturnsNilWhenEmpty() {
        let state = TodoState(items: [])
        XCTAssertNil(state.inProgressItem)
    }

    func testInProgressItemReturnsNilWhenNoneInProgress() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .pending),
            makeTodo(id: "1", status: .done),
        ])
        XCTAssertNil(state.inProgressItem)
    }

    func testInProgressItemReturnsFirstInProgressItem() {
        let state = TodoState(items: [
            makeTodo(id: "0", title: "First", status: .done),
            makeTodo(id: "1", title: "Second", status: .inProgress),
            makeTodo(id: "2", title: "Third", status: .inProgress),
        ])
        XCTAssertEqual(state.inProgressItem?.id, "1")
        XCTAssertEqual(state.inProgressItem?.title, "Second")
    }

    // MARK: - TodoState.isComplete

    func testIsCompleteIsFalseWhenEmpty() {
        let state = TodoState(items: [])
        XCTAssertFalse(state.isComplete)
    }

    func testIsCompleteIsTrueWhenAllItemsDone() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .done),
        ])
        XCTAssertTrue(state.isComplete)
    }

    func testIsCompleteIsTrueWithMixOfDoneAndCompleted() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .completed),
        ])
        XCTAssertTrue(state.isComplete)
    }

    func testIsCompleteIsFalseWhenSomeItemsPending() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .pending),
        ])
        XCTAssertFalse(state.isComplete)
    }

    func testIsCompleteIsFalseWhenAllPending() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .pending),
            makeTodo(id: "1", status: .pending),
        ])
        XCTAssertFalse(state.isComplete)
    }

    func testIsCompleteIsFalseWhenSkippedItemsPresent() {
        let state = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .skipped),
        ])
        XCTAssertFalse(state.isComplete)
    }

    // MARK: - TodoState.hasTodos

    func testHasTodosIsFalseWhenEmpty() {
        let state = TodoState(items: [])
        XCTAssertFalse(state.hasTodos)
    }

    func testHasTodosIsTrueWhenItemsExist() {
        let state = TodoState(items: [makeTodo()])
        XCTAssertTrue(state.hasTodos)
    }

    // MARK: - AggregateTodoStats.isComplete

    func testAggregateIsCompleteWhenAllCompleted() {
        let stats = AggregateTodoStats(completedCount: 5, totalCount: 5)
        XCTAssertTrue(stats.isComplete)
    }

    func testAggregateIsNotCompleteWhenPartiallyDone() {
        let stats = AggregateTodoStats(completedCount: 3, totalCount: 5)
        XCTAssertFalse(stats.isComplete)
    }

    func testAggregateIsNotCompleteWhenEmpty() {
        let stats = AggregateTodoStats.empty
        XCTAssertFalse(stats.isComplete)
    }

    func testAggregateIsNotCompleteWithZeroTotal() {
        let stats = AggregateTodoStats(completedCount: 0, totalCount: 0)
        XCTAssertFalse(stats.isComplete)
    }

    // MARK: - AggregateTodoStats.hasTodos

    func testAggregateHasTodosWhenTotalGreaterThanZero() {
        let stats = AggregateTodoStats(completedCount: 0, totalCount: 3)
        XCTAssertTrue(stats.hasTodos)
    }

    func testAggregateHasNoTodosWhenTotalIsZero() {
        let stats = AggregateTodoStats.empty
        XCTAssertFalse(stats.hasTodos)
    }

    // MARK: - AggregateTodoStats.add(_: AggregateTodoStats) - merging

    func testAddAggregateMergesCounts() {
        var stats = AggregateTodoStats(completedCount: 2, totalCount: 5)
        let other = AggregateTodoStats(completedCount: 3, totalCount: 4)
        stats.add(other)
        XCTAssertEqual(stats.completedCount, 5)
        XCTAssertEqual(stats.totalCount, 9)
    }

    func testAddEmptyAggregateDoesNotChangeCounts() {
        var stats = AggregateTodoStats(completedCount: 2, totalCount: 5)
        stats.add(.empty)
        XCTAssertEqual(stats.completedCount, 2)
        XCTAssertEqual(stats.totalCount, 5)
    }

    func testAddAggregateToEmptyProducesOtherValues() {
        var stats = AggregateTodoStats.empty
        let other = AggregateTodoStats(completedCount: 7, totalCount: 10)
        stats.add(other)
        XCTAssertEqual(stats.completedCount, 7)
        XCTAssertEqual(stats.totalCount, 10)
    }

    func testAddMultipleAggregatesAccumulates() {
        var stats = AggregateTodoStats.empty
        stats.add(AggregateTodoStats(completedCount: 1, totalCount: 2))
        stats.add(AggregateTodoStats(completedCount: 3, totalCount: 4))
        stats.add(AggregateTodoStats(completedCount: 5, totalCount: 6))
        XCTAssertEqual(stats.completedCount, 9)
        XCTAssertEqual(stats.totalCount, 12)
    }

    // MARK: - AggregateTodoStats.add(_: TodoState) - aggregation

    func testAddTodoStateAggregatesCompletedAndTotal() {
        var stats = AggregateTodoStats.empty
        let todoState = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .pending),
            makeTodo(id: "2", status: .completed),
        ])
        stats.add(todoState)
        XCTAssertEqual(stats.completedCount, 2)
        XCTAssertEqual(stats.totalCount, 3)
    }

    func testAddEmptyTodoStateDoesNotChangeCounts() {
        var stats = AggregateTodoStats(completedCount: 5, totalCount: 10)
        stats.add(TodoState(items: []))
        XCTAssertEqual(stats.completedCount, 5)
        XCTAssertEqual(stats.totalCount, 10)
    }

    func testAddTodoStateWithAllDoneItems() {
        var stats = AggregateTodoStats.empty
        let todoState = TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .done),
        ])
        stats.add(todoState)
        XCTAssertEqual(stats.completedCount, 2)
        XCTAssertEqual(stats.totalCount, 2)
        XCTAssertTrue(stats.isComplete)
    }

    func testAddTodoStateWithNoDoneItems() {
        var stats = AggregateTodoStats.empty
        let todoState = TodoState(items: [
            makeTodo(id: "0", status: .pending),
            makeTodo(id: "1", status: .inProgress),
        ])
        stats.add(todoState)
        XCTAssertEqual(stats.completedCount, 0)
        XCTAssertEqual(stats.totalCount, 2)
    }

    func testAddMultipleTodoStatesAccumulates() {
        var stats = AggregateTodoStats.empty

        stats.add(TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .pending),
        ]))

        stats.add(TodoState(items: [
            makeTodo(id: "2", status: .done),
            makeTodo(id: "3", status: .done),
            makeTodo(id: "4", status: .inProgress),
        ]))

        XCTAssertEqual(stats.completedCount, 3)
        XCTAssertEqual(stats.totalCount, 5)
    }

    // MARK: - Mixed add operations

    func testMixedAddAggregateAndTodoState() {
        var stats = AggregateTodoStats(completedCount: 1, totalCount: 2)

        stats.add(TodoState(items: [
            makeTodo(id: "0", status: .done),
            makeTodo(id: "1", status: .pending),
            makeTodo(id: "2", status: .done),
        ]))

        stats.add(AggregateTodoStats(completedCount: 4, totalCount: 6))

        XCTAssertEqual(stats.completedCount, 7)
        XCTAssertEqual(stats.totalCount, 11)
    }
}
