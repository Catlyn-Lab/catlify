import CarPlay
import UIKit

/// CarPlay audio app: tabs for Recent, Library, Browse (charts, genres, moods) and Playlists, plus
/// the system Now Playing screen with Lyrics and Sleep timer buttons.
final class CarPlaySceneDelegate: UIResponder, CPTemplateApplicationSceneDelegate {
    private var interfaceController: CPInterfaceController?
    private let imageCache = NSCache<NSString, UIImage>()
    private var lyricsTimer: Timer?

    func templateApplicationScene(_ scene: CPTemplateApplicationScene,
                                  didConnect interfaceController: CPInterfaceController) {
        NowPlaying.shared.start() // CarPlay may launch the app with no phone UI at all
        self.interfaceController = interfaceController

        let recent = list(title: "Recent", parent: "recent", icon: "clock")
        let library = list(title: "Library", parent: "library", icon: "music.note.list")
        let browse = list(title: "Browse", parent: "browse", icon: "square.grid.2x2")
        let playlists = list(title: "Playlists", parent: "playlists", icon: "list.bullet")
        let tabs = CPTabBarTemplate(templates: [recent, library, browse, playlists])
        interfaceController.setRootTemplate(tabs, animated: false, completion: nil)

        CPNowPlayingTemplate.shared.isUpNextButtonEnabled = false
        CPNowPlayingTemplate.shared.isAlbumArtistButtonEnabled = false
        var buttons: [CPNowPlayingButton] = []
        if let icon = UIImage(systemName: "quote.bubble") {
            buttons.append(CPNowPlayingImageButton(image: icon) { [weak self] _ in self?.showLyrics() })
        }
        if let icon = UIImage(systemName: "moon.zzz") {
            buttons.append(CPNowPlayingImageButton(image: icon) { [weak self] _ in self?.showSleepTimer() })
        }
        CPNowPlayingTemplate.shared.updateNowPlayingButtons(buttons)
    }

    func templateApplicationScene(_ scene: CPTemplateApplicationScene,
                                  didDisconnectInterfaceController interfaceController: CPInterfaceController) {
        self.interfaceController = nil
        lyricsTimer?.invalidate()
    }

    // MARK: Lists

    private func list(title: String, parent: String, icon: String?) -> CPListTemplate {
        let template = CPListTemplate(title: title, sections: sections(for: parent))
        if let icon { template.tabImage = UIImage(systemName: icon) }
        template.emptyViewTitleVariants = ["Nothing here yet"]
        template.emptyViewSubtitleVariants = ["Add music on your iPhone in Catlify"]
        return template
    }

    /// One section per node group ("Genres", "Moods"…), in order of first appearance.
    private func sections(for parent: String) -> [CPListSection] {
        let nodes = Array(StafyEngine.children(parent).prefix(CPListTemplate.maximumItemCount))
        var order: [String?] = []
        for n in nodes where !order.contains(where: { $0 == n.group }) { order.append(n.group) }
        return order.map { group in
            let items = nodes.filter { $0.group == group }.map { item(for: $0, parent: parent) }
            return group.map { CPListSection(items: items, header: $0, sectionIndexTitle: nil) } ?? CPListSection(items: items)
        }
    }

    private func item(for node: StafyEngine.Node, parent: String) -> CPListItem {
        let item = CPListItem(text: node.title, detailText: node.subtitle, image: placeholder(for: node))
        if node.browsable { item.accessoryType = .disclosureIndicator }
        item.handler = { [weak self] _, done in
            self?.select(node, parent: parent)
            done()
        }
        if let art = node.art { loadImage(art) { image in item.setImage(image) } }
        return item
    }

    // MARK: Lyrics (Now Playing button)

    /// The song's lyrics as a list. With synced lyrics the line being sung is marked as playing
    /// and tapping a line jumps there. CarPlay has no free-form text screen, so a list it is.
    private func showLyrics() {
        guard let ic = interfaceController else { return }
        let template = CPListTemplate(title: "Lyrics", sections: [])
        template.emptyViewTitleVariants = ["Loading lyrics…"]
        ic.pushTemplate(template, animated: true, completion: nil)
        loadLyrics(into: template)
    }

    private func loadLyrics(into template: CPListTemplate) {
        lyricsTimer?.invalidate()
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = StafyEngine.call("lyrics") as? [String: Any] ?? [:]
            let lines = result["lines"] as? [[String: Any]] ?? []
            let title = result["title"] as? String
            DispatchQueue.main.async {
                guard let self else { return }
                let items: [CPListItem] = lines.map { line in
                    let time = line["time"] as? Double
                    let item = CPListItem(text: line["text"] as? String ?? "", detailText: nil)
                    item.handler = { _, done in
                        if let time { StafyEngine.call("seek", ["position": time]); StafyEngine.control("play") }
                        done()
                    }
                    return item
                }
                template.emptyViewTitleVariants = ["No lyrics for this song"]
                template.updateSections([CPListSection(items: Array(items.prefix(CPListTemplate.maximumItemCount)))])
                let times = lines.map { $0["time"] as? Double }
                self.followLyrics(template: template, items: items, times: times, title: title)
            }
        }
    }

    /// Marks the current line every half second while the lyrics screen is showing; reloads
    /// when the song changes.
    private func followLyrics(template: CPListTemplate, items: [CPListItem], times: [Double?], title: String?) {
        var current = -1
        lyricsTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] timer in
            guard let self, let ic = self.interfaceController, ic.templates.contains(where: { $0 === template }) else {
                timer.invalidate()
                return
            }
            let state = NowPlaying.shared.state
            let nowTitle = (state["track"] as? [String: Any])?["title"] as? String
            if nowTitle != title {
                self.loadLyrics(into: template)
                return
            }
            guard times.contains(where: { $0 != nil }) else { return }
            let position = state["position"] as? Double ?? 0
            let line = times.lastIndex(where: { ($0 ?? .infinity) <= position + 0.3 }) ?? -1
            if line != current, line < items.count {
                if current >= 0, current < items.count { items[current].isPlaying = false }
                if line >= 0 { items[line].isPlaying = true }
                current = line
            }
        }
    }

    // MARK: Sleep timer (Now Playing button)

    private func showSleepTimer() {
        guard let ic = interfaceController else { return }
        let status = StafyEngine.call("sleepStatus") as? [String: Any] ?? [:]
        var detail = "Off"
        if status["endOfTrack"] as? Bool == true {
            detail = "At the end of this song"
        } else if let endsAt = status["endsAt"] as? Double {
            let minutes = max(1, Int(ceil((endsAt / 1000 - Date().timeIntervalSince1970) / 60)))
            detail = "\(minutes) min left"
        }
        let choose: ([String: Any]) -> Void = { args in
            StafyEngine.call(args.isEmpty ? "sleepCancel" : "sleepSet", args)
            ic.popTemplate(animated: true, completion: nil)
        }
        var items: [CPListItem] = [15, 30, 45, 60, 90].map { m in
            let item = CPListItem(text: "\(m) minutes", detailText: nil)
            item.handler = { _, done in choose(["minutes": m]); done() }
            return item
        }
        let end = CPListItem(text: "End of this song", detailText: nil)
        end.handler = { _, done in choose(["endOfTrack": true]); done() }
        items.append(end)
        if status["active"] as? Bool == true {
            let off = CPListItem(text: "Turn off", detailText: nil)
            off.handler = { _, done in choose([:]); done() }
            items.append(off)
        }
        let template = CPListTemplate(title: "Sleep timer", sections: [CPListSection(items: items, header: detail, sectionIndexTitle: nil)])
        ic.pushTemplate(template, animated: true, completion: nil)
    }

    private func select(_ node: StafyEngine.Node, parent: String) {
        guard let ic = interfaceController else { return }
        if node.browsable {
            let template = CPListTemplate(title: node.title, sections: sections(for: node.id))
            ic.pushTemplate(template, animated: true, completion: nil)
        } else if node.playable {
            StafyEngine.call("play", ["parent": parent, "id": node.id])
            ic.pushTemplate(CPNowPlayingTemplate.shared, animated: true, completion: nil)
        }
    }

    // MARK: Artwork

    private func placeholder(for node: StafyEngine.Node) -> UIImage? {
        let symbol = ["album": "square.stack", "artist": "music.mic", "playlist": "music.note.list",
                      "category": "folder", "track": "music.note"][nodeKind(node)] ?? "music.note"
        return UIImage(systemName: symbol)
    }

    private func nodeKind(_ node: StafyEngine.Node) -> String {
        if node.id.hasPrefix("album:") { return "album" }
        if node.id.hasPrefix("artist:") { return "artist" }
        if node.id.hasPrefix("playlist:") { return "playlist" }
        return node.browsable ? "category" : "track"
    }

    private func loadImage(_ src: String, completion: @escaping (UIImage) -> Void) {
        if let cached = imageCache.object(forKey: src as NSString) {
            completion(cached)
            return
        }
        let finish: (UIImage?) -> Void = { [weak self] image in
            guard let image else { return }
            // Scale down; CarPlay rows are small and memory on head-unit sessions is tight.
            let size = CGSize(width: 120, height: 120)
            let scaled = UIGraphicsImageRenderer(size: size).image { _ in image.draw(in: CGRect(origin: .zero, size: size)) }
            self?.imageCache.setObject(scaled, forKey: src as NSString)
            DispatchQueue.main.async { completion(scaled) }
        }
        if src.hasPrefix("http"), let url = URL(string: src) {
            URLSession.shared.dataTask(with: url) { data, _, _ in finish(data.flatMap(UIImage.init(data:))) }.resume()
        } else {
            DispatchQueue.global(qos: .utility).async { finish(UIImage(contentsOfFile: src)) }
        }
    }
}
