import AppKit
import Foundation

enum WindowControlError: Error, CustomStringConvertible {
    case usage
    case invalidPID(String)
    case noWindow(pid_t)
    case invalidNumber(String)
    case timeout(String)

    var description: String {
        switch self {
        case .usage:
            return "usage: macos-window-control count PID | window-size PID | wait-size PID WIDTH HEIGHT TIMEOUT_MS | fullscreen-state PID | wait-fullscreen PID true|false TIMEOUT_MS"
        case let .invalidPID(value):
            return "invalid PID: \(value)"
        case let .noWindow(pid):
            return "no accessible window for PID \(pid)"
        case let .invalidNumber(value):
            return "invalid numeric value: \(value)"
        case let .timeout(operation):
            return "timed out while waiting for \(operation)"
        }
    }
}

func nativeWindowBounds(pid: pid_t) -> CGRect? {
    let windows = (CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly],
        kCGNullWindowID
    ) as? [[String: Any]]) ?? []
    return windows.compactMap { window -> CGRect? in
        guard (window[kCGWindowOwnerPID as String] as? Int32) == pid,
              (window[kCGWindowLayer as String] as? Int) == 0,
              let dictionary = window[kCGWindowBounds as String] as? [String: Any],
              let x = dictionary["X"] as? NSNumber,
              let y = dictionary["Y"] as? NSNumber,
              let width = dictionary["Width"] as? NSNumber,
              let height = dictionary["Height"] as? NSNumber
        else {
            return nil
        }
        return CGRect(
            x: x.doubleValue,
            y: y.doubleValue,
            width: width.doubleValue,
            height: height.doubleValue
        )
    }.max { first, second in
        first.width * first.height < second.width * second.height
    }
}

func isFullscreen(pid: pid_t) -> Bool {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        return false
    }
    return NSScreen.screens.contains { screen in
        abs(bounds.width - screen.frame.width) < 2
            && bounds.height >= screen.frame.height * 0.9
    }
}

func waitForFullscreen(pid: pid_t, enabled: Bool, timeoutMilliseconds: Int) throws {
    let deadline = Date().addingTimeInterval(Double(timeoutMilliseconds) / 1_000.0)
    while Date() < deadline {
        if isFullscreen(pid: pid) == enabled {
            return
        }
        Thread.sleep(forTimeInterval: 0.1)
    }
    throw WindowControlError.timeout("fullscreen=\(enabled)")
}

func waitForSize(
    pid: pid_t,
    width: Double,
    height: Double,
    timeoutMilliseconds: Int
) throws {
    let deadline = Date().addingTimeInterval(Double(timeoutMilliseconds) / 1_000.0)
    while Date() < deadline {
        if let bounds = nativeWindowBounds(pid: pid),
           abs(bounds.width - width) < 2,
           abs(bounds.height - height) < 2
        {
            return
        }
        Thread.sleep(forTimeInterval: 0.1)
    }
    throw WindowControlError.timeout("size=\(Int(width))x\(Int(height))")
}

func run() throws {
    let arguments = CommandLine.arguments
    guard arguments.count >= 3 else {
        throw WindowControlError.usage
    }
    let command = arguments[1]
    guard let rawPID = Int32(arguments[2]), rawPID > 0 else {
        throw WindowControlError.invalidPID(arguments[2])
    }
    let pid = pid_t(rawPID)

    switch command {
    case "count":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        print(nativeWindowBounds(pid: pid) == nil ? 0 : 1)
    case "window-size":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        guard let bounds = nativeWindowBounds(pid: pid) else {
            throw WindowControlError.noWindow(pid)
        }
        print("\(Int(bounds.width.rounded()))x\(Int(bounds.height.rounded()))")
    case "wait-size":
        guard arguments.count == 6,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let timeoutMilliseconds = Int(arguments[5]), timeoutMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try waitForSize(
            pid: pid,
            width: width,
            height: height,
            timeoutMilliseconds: timeoutMilliseconds
        )
    case "fullscreen-state":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        print(isFullscreen(pid: pid) ? "true" : "false")
    case "wait-fullscreen":
        guard arguments.count == 5,
              arguments[3] == "true" || arguments[3] == "false",
              let timeoutMilliseconds = Int(arguments[4]), timeoutMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try waitForFullscreen(
            pid: pid,
            enabled: arguments[3] == "true",
            timeoutMilliseconds: timeoutMilliseconds
        )
    default:
        throw WindowControlError.usage
    }
}

do {
    try run()
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(1)
}
