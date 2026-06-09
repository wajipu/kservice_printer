// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "kservice_printer",
    platforms: [
        .macOS("10.15"),
    ],
    products: [
        .library(name: "kservice-printer", targets: ["kservice_printer"])
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework")
    ],
    targets: [
        .target(
            name: "kservice_printer",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework")
            ],
            sources: ["dummy.c"]
        )
    ]
)
