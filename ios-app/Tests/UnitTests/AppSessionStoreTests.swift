import XCTest
@testable import TenexMVP

@MainActor
final class AppSessionStoreTests: XCTestCase {
    func testApplyAutoLoginResultNoCredentialsKeepsLoggedOutState() async {
        let credentials = MockCredentialStore()
        let store = AppSessionStore(credentials: credentials)

        store.applyAutoLoginResult(.noCredentials)

        XCTAssertFalse(store.isLoggedIn)
        XCTAssertEqual(store.userNpub, "")
        XCTAssertNil(store.autoLoginError)
        XCTAssertEqual(credentials.deleteCalls, 0)
    }

    func testApplyAutoLoginResultSuccessSetsSessionState() async {
        let store = AppSessionStore(credentials: MockCredentialStore())
        store.applyAutoLoginResult(.success(npub: "npub1testuser"))

        XCTAssertTrue(store.isLoggedIn)
        XCTAssertEqual(store.userNpub, "npub1testuser")
        XCTAssertNil(store.autoLoginError)
    }

    func testApplyAutoLoginResultInvalidCredentialSetsErrorAndDeletesCredential() async {
        let credentials = MockCredentialStore()
        let store = AppSessionStore(credentials: credentials)

        store.applyAutoLoginResult(.invalidCredential(error: "bad nsec"))
        await Task.yield()

        XCTAssertEqual(
            store.autoLoginError,
            "Stored credential was invalid. Please log in again."
        )
        XCTAssertEqual(credentials.deleteCalls, 1)
    }

    func testApplyAutoLoginResultTransientErrorSetsUserFacingError() async {
        let store = AppSessionStore(credentials: MockCredentialStore())

        store.applyAutoLoginResult(.transientError(error: "relay timeout"))

        XCTAssertFalse(store.isLoggedIn)
        XCTAssertEqual(store.autoLoginError, "Could not auto-login: relay timeout")
    }
}

private final class MockCredentialStore: CredentialStoring {
    var deleteCalls = 0

    func loadNsec() -> KeychainResult<String> {
        .failure(.itemNotFound)
    }

    func loadNsecAsync() async -> KeychainResult<String> {
        .failure(.itemNotFound)
    }

    func saveNsecAsync(_: String) async -> KeychainResult<Void> {
        .success(())
    }

    func deleteNsecAsync() async -> KeychainResult<Void> {
        deleteCalls += 1
        return .success(())
    }
}
