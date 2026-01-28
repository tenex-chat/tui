import ProjectDescription

let project = Project(
    name: "TenexMVP",
    targets: [
        .target(
            name: "TenexMVP",
            destinations: [.iPhone, .iPad],
            product: .app,
            bundleId: "com.tenex.mvp",
            deploymentTargets: .iOS("17.0"),
            infoPlist: .extendingDefault(with: [
                "UILaunchScreen": [
                    "UIColorName": "",
                    "UIImageName": ""
                ],
                "CFBundleDisplayName": "TENEX MVP"
            ]),
            sources: ["Sources/TenexMVP/**/*.swift"],
            resources: ["Sources/TenexMVP/Resources/**"],
            settings: .settings(
                base: [
                    // Code signing settings
                    "DEVELOPMENT_TEAM": "B3YMNJ5848",
                    "CODE_SIGN_STYLE": "Automatic",
                    // Header search paths for the FFI header
                    "HEADER_SEARCH_PATHS": [
                        "$(inherited)",
                        "$(SRCROOT)/Sources/TenexMVP/TenexCoreFFI"
                    ],
                    // Library search path for libtenex_core.a
                    "LIBRARY_SEARCH_PATHS[sdk=iphonesimulator*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios-sim/release"
                    ],
                    "LIBRARY_SEARCH_PATHS[sdk=iphoneos*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios/release"
                    ],
                    // Link the Rust static library - use full path to force static linking
                    "OTHER_LDFLAGS[sdk=iphonesimulator*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios-sim/release/libtenex_core.a"
                    ],
                    "OTHER_LDFLAGS[sdk=iphoneos*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios/release/libtenex_core.a"
                    ],
                    // Swift import paths for the modulemap
                    "SWIFT_INCLUDE_PATHS": [
                        "$(inherited)",
                        "$(SRCROOT)/Sources/TenexMVP/TenexCoreFFI"
                    ],
                    // Disable auto-linking of UIUtilities and SwiftUICore frameworks
                    "OTHER_SWIFT_FLAGS": [
                        "$(inherited)",
                        "-Xfrontend", "-disable-autolink-framework", "-Xfrontend", "UIUtilities",
                        "-Xfrontend", "-disable-autolink-framework", "-Xfrontend", "SwiftUICore"
                    ]
                ]
            )
        )
    ]
)
