import ObjectiveC
import UIKit

/// Called from main.mm before Tauri starts.
@_cdecl("stafy_ios_bootstrap")
public func stafyIosBootstrap() {
    // Before Tauri starts, so the engine finds yt-dlp for the first YouTube song.
    YTDLPBridge.register()
    NotificationCenter.default.addObserver(forName: UIApplication.didFinishLaunchingNotification, object: nil, queue: .main) { _ in
        routeCarPlayScenes()
        NowPlaying.shared.start()
    }
}

/// Tauri's iOS runtime (tao) answers `application(_:configurationForConnecting:options:)` and assigns
/// its own window-scene delegate to *every* scene. Wrap it so CarPlay sessions get our delegate.
private func routeCarPlayScenes() {
    guard let delegate = UIApplication.shared.delegate, let cls = object_getClass(delegate) else { return }
    let sel = #selector(UIApplicationDelegate.application(_:configurationForConnecting:options:))
    typealias Original = @convention(c) (AnyObject, Selector, UIApplication, UISceneSession, UIScene.ConnectionOptions) -> UISceneConfiguration
    let carPlay: (UISceneSession) -> UISceneConfiguration = { session in
        let config = UISceneConfiguration(name: "CarPlay", sessionRole: session.role)
        config.delegateClass = CarPlaySceneDelegate.self
        return config
    }
    if let method = class_getInstanceMethod(cls, sel) {
        let original = unsafeBitCast(method_getImplementation(method), to: Original.self)
        let block: @convention(block) (AnyObject, UIApplication, UISceneSession, UIScene.ConnectionOptions) -> UISceneConfiguration = {
            obj, app, session, options in
            session.role == .carTemplateApplication ? carPlay(session) : original(obj, sel, app, session, options)
        }
        method_setImplementation(method, imp_implementationWithBlock(block))
    }
}
