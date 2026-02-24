import ProjectDescription

let project = Project(
    name: "TenexMVP",
    packages: [
        .remote(url: "https://github.com/onevcat/Kingfisher.git", requirement: .upToNextMajor(from: "8.1.0"))
    ],
    targets: [
        .target(
            name: "TenexMVP",
            destinations: [.iPhone, .iPad, .mac],
            product: .app,
            bundleId: "com.tenex.mvp",
            deploymentTargets: .multiplatform(iOS: "26.0", macOS: "15.0"),
            infoPlist: .extendingDefault(with: [
                "UILaunchScreen": [
                    "UIColorName": "",
                    "UIImageName": ""
                ],
                "CFBundleDisplayName": "TENEX",
                "CFBundleIconFile": "AppIcon",
                "CFBundleIconName": "AppIcon",
                "NSMicrophoneUsageDescription": "TENEX needs microphone access for voice dictation",
                "NSSpeechRecognitionUsageDescription": "TENEX uses speech recognition for voice-to-text dictation",
                "NSUserNotificationsUsageDescription": "TENEX sends notifications when agents ask questions that need your attention",
                "UIBackgroundModes": ["audio", "fetch", "remote-notification"],
                "CFBundleURLTypes": [
                    [
                        "CFBundleURLSchemes": ["tenex"],
                        "CFBundleURLName": "com.tenex.mvp"
                    ]
                ]
            ]),
            sources: [
                .glob("Sources/TenexMVP/**/*.swift")
            ],
            resources: ["Sources/TenexMVP/Resources/**"],
            entitlements: .file(path: "TenexMVP.entitlements"),
            scripts: [
                .pre(
                    script: "bash \"${SRCROOT}/../scripts/generate-swift-bindings.sh\"",
                    name: "Generate Swift Bindings",
                    basedOnDependencyAnalysis: false
                )
            ],
            dependencies: [
                .package(product: "Kingfisher", type: .runtime),
                .sdk(name: "AppIntents", type: .framework)
            ],
            settings: .settings(
                base: [
                    // Code signing settings - SANITY ISLAND LLC
                    "DEVELOPMENT_TEAM": "456SHKPP26",
                    "CODE_SIGN_STYLE": "Automatic",
                    // Header search paths for the FFI header
                    "HEADER_SEARCH_PATHS": [
                        "$(inherited)",
                        "$(SRCROOT)/Sources/TenexMVP/TenexCoreFFI"
                    ],
                    // Library search path for libtenex_core.a
                    "LIBRARY_SEARCH_PATHS[sdk=iphonesimulator*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/universal-ios-sim/release"
                    ],
                    "LIBRARY_SEARCH_PATHS[sdk=iphoneos*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios/release"
                    ],
                    "LIBRARY_SEARCH_PATHS[sdk=macosx*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/release"
                    ],
                    // Link the Rust static library - use full path to force static linking
                    "OTHER_LDFLAGS[sdk=iphonesimulator*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/universal-ios-sim/release/libtenex_core.a"
                    ],
                    "OTHER_LDFLAGS[sdk=iphoneos*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/aarch64-apple-ios/release/libtenex_core.a"
                    ],
                    "OTHER_LDFLAGS[sdk=macosx*]": [
                        "$(inherited)",
                        "$(SRCROOT)/../target/release/libtenex_core.a",
                        "-framework", "SystemConfiguration"
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
        ),
        .target(
            name: "TenexMVPTests",
            destinations: [.mac],
            product: .unitTests,
            bundleId: "com.tenex.mvp.tests",
            deploymentTargets: .macOS("15.0"),
            infoPlist: .default,
            sources: ["Tests/UnitTests/**/*.swift"],
            dependencies: [
                .target(name: "TenexMVP")
            ],
            settings: .settings(
                base: [
                    "DEVELOPMENT_TEAM": "456SHKPP26",
                    "CODE_SIGN_STYLE": "Automatic"
                ]
            )
        ),
        .target(
            name: "TenexMVPUITests",
            destinations: [.iPhone, .iPad],
            product: .uiTests,
            bundleId: "com.tenex.mvp.uitests",
            deploymentTargets: .iOS("26.0"),
            infoPlist: .default,
            sources: ["Tests/UITests/**/*.swift"],
            dependencies: [
                .target(name: "TenexMVP")
            ],
            settings: .settings(
                base: [
                    "DEVELOPMENT_TEAM": "456SHKPP26",
                    "CODE_SIGN_STYLE": "Automatic"
                ]
            )
        )
    ],
    schemes: [
        .scheme(
            name: "TenexMVP-UnitTests",
            shared: true,
            buildAction: .buildAction(targets: ["TenexMVP", "TenexMVPTests"]),
            testAction: .targets(
                ["TenexMVPTests"],
                configuration: .debug,
                options: .options(coverage: true, codeCoverageTargets: ["TenexMVP"])
            ),
            runAction: .runAction(configuration: .debug, executable: "TenexMVP"),
            archiveAction: .archiveAction(configuration: .release),
            profileAction: .profileAction(configuration: .release, executable: "TenexMVP"),
            analyzeAction: .analyzeAction(configuration: .debug)
        )
    ]
)
