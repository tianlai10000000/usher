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
            return "usage: macos-window-control count PID | window-list PID | window-id PID | window-size PID | smallest-window-size PID | move-smallest-window PID X Y | ax-resize-smallest-cycle PID WIDTH HEIGHT STEPS INTERVAL_MS | wait-size PID WIDTH HEIGHT TIMEOUT_MS | wait-smallest-size PID WIDTH HEIGHT TIMEOUT_MS | fullscreen-state PID | wait-fullscreen PID true|false TIMEOUT_MS | click-burst PID OFFSET_X OFFSET_Y COUNT INTERVAL_MS | pointer-drag PID OFFSET_X OFFSET_Y DELTA_X DELTA_Y STEPS INTERVAL_MS | pointer-drag-cycle PID OFFSET_X OFFSET_Y DELTA_X STEPS INTERVAL_MS | resize-drag PID WIDTH HEIGHT STEPS INTERVAL_MS | resize-cycle PID WIDTH HEIGHT STEPS INTERVAL_MS | resize-smallest-drag PID WIDTH HEIGHT STEPS INTERVAL_MS | resize-smallest-cycle PID WIDTH HEIGHT STEPS INTERVAL_MS | scroll-burst PID OFFSET_X OFFSET_Y DELTA COUNT INTERVAL_MS | scroll-cycle PID OFFSET_X OFFSET_Y DELTA COUNT INTERVAL_MS"
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

func smallestNativeWindowBounds(pid: pid_t) -> CGRect? {
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
    }.min { first, second in
        first.width * first.height < second.width * second.height
    }
}

func selectedPreviewAccessibilityWindow(pid: pid_t) throws -> AXUIElement {
    let application = AXUIElementCreateApplication(pid)
    var rawWindows: CFTypeRef?
    guard AXUIElementCopyAttributeValue(
        application,
        kAXWindowsAttribute as CFString,
        &rawWindows
    ) == .success,
        let windows = rawWindows as? [AXUIElement]
    else {
        throw WindowControlError.noWindow(pid)
    }
    let candidates = windows.compactMap { window -> (AXUIElement, Double, String)? in
        var rawSize: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            window,
            kAXSizeAttribute as CFString,
            &rawSize
        ) == .success,
            let value = rawSize,
            CFGetTypeID(value) == AXValueGetTypeID()
        else {
            return nil
        }
        var size = CGSize.zero
        guard AXValueGetValue(value as! AXValue, .cgSize, &size) else {
            return nil
        }
        var rawTitle: CFTypeRef?
        AXUIElementCopyAttributeValue(
            window,
            kAXTitleAttribute as CFString,
            &rawTitle
        )
        return (
            window,
            size.width * size.height,
            rawTitle as? String ?? ""
        )
    }
    let selected = candidates.first { candidate in
        candidate.2 == "Artifact preview benchmark"
    } ?? candidates.min { first, second in
        first.1 < second.1
    }
    guard let window = selected?.0 else {
        throw WindowControlError.noWindow(pid)
    }
    return window
}

func moveSmallestAccessibilityWindow(pid: pid_t, x: Double, y: Double) throws {
    let window = try selectedPreviewAccessibilityWindow(pid: pid)
    var position = CGPoint(x: x, y: y)
    guard let value = AXValueCreate(.cgPoint, &position),
          AXUIElementSetAttributeValue(
              window,
              kAXPositionAttribute as CFString,
              value
          ) == .success
    else {
        throw WindowControlError.timeout("move-smallest-window")
    }
}

func resizeSmallestAccessibilityCycle(
    pid: pid_t,
    width: Double,
    height: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    let window = try selectedPreviewAccessibilityWindow(pid: pid)
    var rawSize: CFTypeRef?
    guard AXUIElementCopyAttributeValue(
        window,
        kAXSizeAttribute as CFString,
        &rawSize
    ) == .success,
        let value = rawSize,
        CFGetTypeID(value) == AXValueGetTypeID()
    else {
        throw WindowControlError.noWindow(pid)
    }
    var startSize = CGSize.zero
    guard AXValueGetValue(value as! AXValue, .cgSize, &startSize) else {
        throw WindowControlError.noWindow(pid)
    }
    for leg in 0 ... 1 {
        for step in 1 ... steps {
            let rawProgress = Double(step) / Double(steps)
            let progress = leg == 0 ? rawProgress : 1.0 - rawProgress
            var size = CGSize(
                width: startSize.width + (width - startSize.width) * progress,
                height: height
            )
            guard let sizeValue = AXValueCreate(.cgSize, &size),
                  AXUIElementSetAttributeValue(
                      window,
                      kAXSizeAttribute as CFString,
                      sizeValue
                  ) == .success
            else {
                throw WindowControlError.timeout("ax-resize-smallest-cycle")
            }
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
}

func nativeWindowID(pid: pid_t) -> CGWindowID? {
    let windows = (CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly],
        kCGNullWindowID
    ) as? [[String: Any]]) ?? []
    return windows.compactMap { window -> (CGWindowID, Double)? in
        guard (window[kCGWindowOwnerPID as String] as? Int32) == pid,
              (window[kCGWindowLayer as String] as? Int) == 0,
              let id = window[kCGWindowNumber as String] as? NSNumber,
              let dictionary = window[kCGWindowBounds as String] as? [String: Any],
              let width = dictionary["Width"] as? NSNumber,
              let height = dictionary["Height"] as? NSNumber
        else {
            return nil
        }
        return (CGWindowID(id.uint32Value), width.doubleValue * height.doubleValue)
    }.max { first, second in
        first.1 < second.1
    }?.0
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

func waitForSmallestSize(
    pid: pid_t,
    width: Double,
    height: Double,
    timeoutMilliseconds: Int
) throws {
    let deadline = Date().addingTimeInterval(Double(timeoutMilliseconds) / 1_000.0)
    while Date() < deadline {
        if let bounds = smallestNativeWindowBounds(pid: pid),
           abs(bounds.width - width) < 2,
           abs(bounds.height - height) < 2
        {
            return
        }
        Thread.sleep(forTimeInterval: 0.1)
    }
    throw WindowControlError.timeout("smallest-size=\(Int(width))x\(Int(height))")
}

func clickBurst(
    pid: pid_t,
    offsetX: Double,
    offsetY: Double,
    count: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    let point = CGPoint(x: bounds.minX + offsetX, y: bounds.minY + offsetY)
    for index in 0 ..< count {
        CGEvent(
            mouseEventSource: nil,
            mouseType: .leftMouseDown,
            mouseCursorPosition: point,
            mouseButton: .left
        )?.post(tap: .cghidEventTap)
        CGEvent(
            mouseEventSource: nil,
            mouseType: .leftMouseUp,
            mouseCursorPosition: point,
            mouseButton: .left
        )?.post(tap: .cghidEventTap)
        if index + 1 < count {
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
}

func pointerDrag(
    pid: pid_t,
    offsetX: Double,
    offsetY: Double,
    deltaX: Double,
    deltaY: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    let start = CGPoint(x: bounds.minX + offsetX, y: bounds.minY + offsetY)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for step in 1 ... steps {
        let progress = Double(step) / Double(steps)
        let point = CGPoint(
            x: start.x + deltaX * progress,
            y: start.y + deltaY * progress
        )
        CGEvent(
            mouseEventSource: nil,
            mouseType: .leftMouseDragged,
            mouseCursorPosition: point,
            mouseButton: .left
        )?.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
    }
    let end = CGPoint(x: start.x + deltaX, y: start.y + deltaY)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: end,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func pointerDragCycle(
    pid: pid_t,
    offsetX: Double,
    offsetY: Double,
    deltaX: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    let start = CGPoint(x: bounds.minX + offsetX, y: bounds.minY + offsetY)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for leg in 0 ... 1 {
        for step in 1 ... steps {
            let rawProgress = Double(step) / Double(steps)
            let progress = leg == 0 ? rawProgress : 1.0 - rawProgress
            let point = CGPoint(x: start.x + deltaX * progress, y: start.y)
            CGEvent(
                mouseEventSource: nil,
                mouseType: .leftMouseDragged,
                mouseCursorPosition: point,
                mouseButton: .left
            )?.post(tap: .cghidEventTap)
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func resizeDrag(
    pid: pid_t,
    width: Double,
    height: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)

    let start = CGPoint(x: bounds.maxX - 2, y: bounds.maxY - 2)
    let target = CGPoint(x: bounds.minX + width - 2, y: bounds.minY + height - 2)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for step in 1 ... steps {
        let progress = Double(step) / Double(steps)
        let point = CGPoint(
            x: start.x + (target.x - start.x) * progress,
            y: start.y + (target.y - start.y) * progress
        )
        CGEvent(
            mouseEventSource: nil,
            mouseType: .leftMouseDragged,
            mouseCursorPosition: point,
            mouseButton: .left
        )?.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
    }
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: target,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func resizeCycle(
    pid: pid_t,
    width: Double,
    height: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)

    let start = CGPoint(x: bounds.maxX - 2, y: bounds.maxY - 2)
    let target = CGPoint(x: bounds.minX + width - 2, y: bounds.minY + height - 2)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for leg in 0 ... 1 {
        for step in 1 ... steps {
            let rawProgress = Double(step) / Double(steps)
            let progress = leg == 0 ? rawProgress : 1.0 - rawProgress
            let point = CGPoint(
                x: start.x + (target.x - start.x) * progress,
                y: start.y + (target.y - start.y) * progress
            )
            CGEvent(
                mouseEventSource: nil,
                mouseType: .leftMouseDragged,
                mouseCursorPosition: point,
                mouseButton: .left
            )?.post(tap: .cghidEventTap)
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func resizeSmallestCycle(
    pid: pid_t,
    width: Double,
    height: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = smallestNativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)

    let start = CGPoint(x: bounds.maxX - 2, y: bounds.maxY - 2)
    let target = CGPoint(x: bounds.minX + width - 2, y: bounds.minY + height - 2)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for leg in 0 ... 1 {
        for step in 1 ... steps {
            let rawProgress = Double(step) / Double(steps)
            let progress = leg == 0 ? rawProgress : 1.0 - rawProgress
            let point = CGPoint(
                x: start.x + (target.x - start.x) * progress,
                y: start.y + (target.y - start.y) * progress
            )
            CGEvent(
                mouseEventSource: nil,
                mouseType: .leftMouseDragged,
                mouseCursorPosition: point,
                mouseButton: .left
            )?.post(tap: .cghidEventTap)
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func resizeSmallestDrag(
    pid: pid_t,
    width: Double,
    height: Double,
    steps: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = smallestNativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)

    let start = CGPoint(x: bounds.maxX - 2, y: bounds.maxY - 2)
    let target = CGPoint(x: bounds.minX + width - 2, y: bounds.minY + height - 2)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.12)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseDown,
        mouseCursorPosition: start,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
    for step in 1 ... steps {
        let progress = Double(step) / Double(steps)
        let point = CGPoint(
            x: start.x + (target.x - start.x) * progress,
            y: start.y + (target.y - start.y) * progress
        )
        CGEvent(
            mouseEventSource: nil,
            mouseType: .leftMouseDragged,
            mouseCursorPosition: point,
            mouseButton: .left
        )?.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
    }
    // Keep the final edge position down long enough for a heavily blocked
    // target process to consume the last drag event before mouse-up.
    Thread.sleep(forTimeInterval: 0.5)
    CGEvent(
        mouseEventSource: nil,
        mouseType: .leftMouseUp,
        mouseCursorPosition: target,
        mouseButton: .left
    )?.post(tap: .cghidEventTap)
}

func postScrollBurst(
    point: CGPoint,
    delta: Int32,
    count: Int,
    intervalMilliseconds: Int
) {
    for index in 0 ..< count {
        let event = CGEvent(
            scrollWheelEvent2Source: nil,
            units: .pixel,
            wheelCount: 1,
            wheel1: delta,
            wheel2: 0,
            wheel3: 0
        )
        event?.location = point
        event?.post(tap: .cghidEventTap)
        if index + 1 < count {
            Thread.sleep(forTimeInterval: Double(intervalMilliseconds) / 1_000.0)
        }
    }
}

func scrollBurst(
    pid: pid_t,
    offsetX: Double,
    offsetY: Double,
    delta: Int32,
    count: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    let point = CGPoint(x: bounds.minX + offsetX, y: bounds.minY + offsetY)
    postScrollBurst(
        point: point,
        delta: delta,
        count: count,
        intervalMilliseconds: intervalMilliseconds
    )
}

func scrollCycle(
    pid: pid_t,
    offsetX: Double,
    offsetY: Double,
    delta: Int32,
    count: Int,
    intervalMilliseconds: Int
) throws {
    guard let bounds = nativeWindowBounds(pid: pid) else {
        throw WindowControlError.noWindow(pid)
    }
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    let point = CGPoint(x: bounds.minX + offsetX, y: bounds.minY + offsetY)
    postScrollBurst(
        point: point,
        delta: delta,
        count: count,
        intervalMilliseconds: intervalMilliseconds
    )
    postScrollBurst(
        point: point,
        delta: -delta,
        count: count,
        intervalMilliseconds: intervalMilliseconds
    )
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
    case "window-list":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        let windows = (CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly],
            kCGNullWindowID
        ) as? [[String: Any]]) ?? []
        for window in windows where
            (window[kCGWindowOwnerPID as String] as? Int32) == pid
                && (window[kCGWindowLayer as String] as? Int) == 0
        {
            let id = (window[kCGWindowNumber as String] as? NSNumber)?.uint32Value ?? 0
            let name = window[kCGWindowName as String] as? String ?? ""
            guard let dictionary = window[kCGWindowBounds as String] as? [String: Any],
                  let x = dictionary["X"] as? NSNumber,
                  let y = dictionary["Y"] as? NSNumber,
                  let width = dictionary["Width"] as? NSNumber,
                  let height = dictionary["Height"] as? NSNumber
            else {
                continue
            }
            print("\(id)\t\(name)\t\(Int(x.doubleValue))\t\(Int(y.doubleValue))\t\(Int(width.doubleValue.rounded()))\t\(Int(height.doubleValue.rounded()))")
        }
    case "window-id":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        guard let id = nativeWindowID(pid: pid) else {
            throw WindowControlError.noWindow(pid)
        }
        print(id)
    case "window-size":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        guard let bounds = nativeWindowBounds(pid: pid) else {
            throw WindowControlError.noWindow(pid)
        }
        print("\(Int(bounds.width.rounded()))x\(Int(bounds.height.rounded()))")
    case "smallest-window-size":
        guard arguments.count == 3 else {
            throw WindowControlError.usage
        }
        guard let bounds = smallestNativeWindowBounds(pid: pid) else {
            throw WindowControlError.noWindow(pid)
        }
        print("\(Int(bounds.width.rounded()))x\(Int(bounds.height.rounded()))")
    case "move-smallest-window":
        guard arguments.count == 5,
              let x = Double(arguments[3]),
              let y = Double(arguments[4])
        else {
            throw WindowControlError.usage
        }
        try moveSmallestAccessibilityWindow(pid: pid, x: x, y: y)
    case "ax-resize-smallest-cycle":
        guard arguments.count == 7,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let steps = Int(arguments[5]), steps > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try resizeSmallestAccessibilityCycle(
            pid: pid,
            width: width,
            height: height,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
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
    case "wait-smallest-size":
        guard arguments.count == 6,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let timeoutMilliseconds = Int(arguments[5]), timeoutMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try waitForSmallestSize(
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
    case "click-burst":
        guard arguments.count == 7,
              let offsetX = Double(arguments[3]),
              let offsetY = Double(arguments[4]),
              let count = Int(arguments[5]), count > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try clickBurst(
            pid: pid,
            offsetX: offsetX,
            offsetY: offsetY,
            count: count,
            intervalMilliseconds: intervalMilliseconds
        )
    case "pointer-drag-cycle":
        guard arguments.count == 8,
              let offsetX = Double(arguments[3]),
              let offsetY = Double(arguments[4]),
              let deltaX = Double(arguments[5]), deltaX != 0,
              let steps = Int(arguments[6]), steps > 0,
              let intervalMilliseconds = Int(arguments[7]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try pointerDragCycle(
            pid: pid,
            offsetX: offsetX,
            offsetY: offsetY,
            deltaX: deltaX,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "pointer-drag":
        guard arguments.count == 9,
              let offsetX = Double(arguments[3]),
              let offsetY = Double(arguments[4]),
              let deltaX = Double(arguments[5]),
              let deltaY = Double(arguments[6]),
              deltaX != 0 || deltaY != 0,
              let steps = Int(arguments[7]), steps > 0,
              let intervalMilliseconds = Int(arguments[8]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try pointerDrag(
            pid: pid,
            offsetX: offsetX,
            offsetY: offsetY,
            deltaX: deltaX,
            deltaY: deltaY,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "resize-drag":
        guard arguments.count == 7,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let steps = Int(arguments[5]), steps > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try resizeDrag(
            pid: pid,
            width: width,
            height: height,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "resize-cycle":
        guard arguments.count == 7,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let steps = Int(arguments[5]), steps > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try resizeCycle(
            pid: pid,
            width: width,
            height: height,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "resize-smallest-cycle":
        guard arguments.count == 7,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let steps = Int(arguments[5]), steps > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try resizeSmallestCycle(
            pid: pid,
            width: width,
            height: height,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "resize-smallest-drag":
        guard arguments.count == 7,
              let width = Double(arguments[3]), width > 0,
              let height = Double(arguments[4]), height > 0,
              let steps = Int(arguments[5]), steps > 0,
              let intervalMilliseconds = Int(arguments[6]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try resizeSmallestDrag(
            pid: pid,
            width: width,
            height: height,
            steps: steps,
            intervalMilliseconds: intervalMilliseconds
        )
    case "scroll-burst":
        guard arguments.count == 8,
              let offsetX = Double(arguments[3]),
              let offsetY = Double(arguments[4]),
              let delta = Int32(arguments[5]), delta != 0,
              let count = Int(arguments[6]), count > 0,
              let intervalMilliseconds = Int(arguments[7]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try scrollBurst(
            pid: pid,
            offsetX: offsetX,
            offsetY: offsetY,
            delta: delta,
            count: count,
            intervalMilliseconds: intervalMilliseconds
        )
    case "scroll-cycle":
        guard arguments.count == 8,
              let offsetX = Double(arguments[3]),
              let offsetY = Double(arguments[4]),
              let delta = Int32(arguments[5]), delta != 0,
              let count = Int(arguments[6]), count > 0,
              let intervalMilliseconds = Int(arguments[7]), intervalMilliseconds > 0
        else {
            throw WindowControlError.usage
        }
        try scrollCycle(
            pid: pid,
            offsetX: offsetX,
            offsetY: offsetY,
            delta: delta,
            count: count,
            intervalMilliseconds: intervalMilliseconds
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
