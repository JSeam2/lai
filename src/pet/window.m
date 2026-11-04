#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>

#include <stdbool.h>
#include <stdint.h>

typedef bool (*PetCheck)(void *);
typedef void (*PetStop)(void *);

@interface PetWindow : NSWindow
@end
@implementation PetWindow
- (BOOL)canBecomeKeyWindow { return YES; }
@end

@interface PetController : NSObject <WKScriptMessageHandler, WKNavigationDelegate>
@property(nonatomic, strong) PetWindow *window;
@property(nonatomic, strong) WKWebView *webView;
@property(nonatomic, strong) NSTimer *timer;
@property(nonatomic, assign) void *context;
@property(nonatomic, assign) PetCheck shouldStop;
@property(nonatomic, assign) PetCheck serverDone;
@property(nonatomic, assign) PetStop requestStop;
@property(nonatomic, assign) PetStop markReady;
@property(nonatomic, assign) uint16_t apiPort;
@property(nonatomic, assign) uint16_t uiPort;
@property(nonatomic, assign) NSPoint dragOrigin;
@property(nonatomic, assign) NSTimeInterval sleepStarted;
@property(nonatomic, assign) NSTimeInterval started;
@property(nonatomic, assign) BOOL uiReady;
@property(nonatomic, assign) BOOL uiFailed;
@end

@implementation PetController
- (void)start:(uint16_t)uiPort {
    self.uiPort = uiPort;
    self.started = NSProcessInfo.processInfo.systemUptime;
    NSRect frame = NSMakeRect(100, 100, 245, 300);
    NSScreen *screen = NSScreen.mainScreen;
    if (screen) {
        NSRect visible = screen.visibleFrame;
        frame.origin = NSMakePoint(NSMaxX(visible) - frame.size.width - 20,
                                   NSMinY(visible) + 20);
    }
    self.window = [[PetWindow alloc] initWithContentRect:frame styleMask:NSWindowStyleMaskBorderless
                                                  backing:NSBackingStoreBuffered defer:NO];
    self.window.opaque = NO;
    self.window.backgroundColor = NSColor.clearColor;
    self.window.hasShadow = NO;
    self.window.level = NSFloatingWindowLevel;
    self.window.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    WKWebViewConfiguration *config = [WKWebViewConfiguration new];
    config.websiteDataStore = WKWebsiteDataStore.nonPersistentDataStore;
    [config.userContentController addScriptMessageHandler:self name:@"pet"];
    self.webView = [[WKWebView alloc] initWithFrame:NSMakeRect(0, 0, 245, 300) configuration:config];
    [self.webView setValue:@NO forKey:@"drawsBackground"];
    self.webView.navigationDelegate = self;
    self.webView.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.window.contentView = self.webView;
    [self.webView loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:
        [NSString stringWithFormat:@"http://127.0.0.1:%u/", uiPort]]]];
    [self.window makeKeyAndOrderFront:nil];
    self.timer = [NSTimer scheduledTimerWithTimeInterval:0.05 target:self selector:@selector(tick:) userInfo:nil repeats:YES];
}

- (void)fail:(NSString *)reason {
    if (self.uiFailed) return;
    self.uiFailed = YES;
    fprintf(stderr, "Pet failed to start: %s\n", reason.UTF8String);
    self.requestStop(self.context);
}

- (void)webView:(WKWebView *)webView didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {
    (void)webView; (void)navigation;
    [self fail:error.localizedDescription];
}

- (void)tick:(NSTimer *)timer {
    (void)timer;
    if (!self.uiReady && !self.shouldStop(self.context) && NSProcessInfo.processInfo.systemUptime - self.started > 30) {
        [self fail:@"the renderer did not become ready"];
    }
    if (self.shouldStop(self.context) && self.sleepStarted == 0) {
        self.sleepStarted = NSProcessInfo.processInfo.systemUptime;
        [self.webView evaluateJavaScript:@"window.laiSleep && window.laiSleep()" completionHandler:nil];
    }
    if (self.sleepStarted && self.serverDone(self.context) &&
        NSProcessInfo.processInfo.systemUptime - self.sleepStarted >= 1.9) {
        [self.timer invalidate];
        [self.window orderOut:nil];
        [NSApp stop:nil];
        [NSApp postEvent:[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint
            modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0]
            atStart:NO];
    }
}

- (void)sleep:(id)sender {
    (void)sender;
    self.requestStop(self.context);
}

- (void)showMenu {
    NSMenu *menu = [NSMenu new];
    NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:@"Tell pet to go away"
                                                  action:@selector(sleep:) keyEquivalent:@""];
    item.target = self;
    [menu addItem:item];
    [menu popUpMenuPositioningItem:nil atLocation:[self.webView convertPoint:self.window.mouseLocationOutsideOfEventStream fromView:nil]
                            inView:self.webView];
}

- (void)reply:(NSDictionary *)result {
    NSData *data = [NSJSONSerialization dataWithJSONObject:result options:NSJSONWritingFragmentsAllowed error:nil];
    if (!data) return;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    dispatch_async(dispatch_get_main_queue(), ^{
        [self.webView evaluateJavaScript:[@"window.laiResponse && window.laiResponse(" stringByAppendingFormat:@"%@)", json]
                       completionHandler:nil];
    });
}

- (void)chat:(NSArray *)messages {
    NSURL *modelsURL = [NSURL URLWithString:[NSString stringWithFormat:@"http://127.0.0.1:%u/v1/models", self.apiPort]];
    NSURLSession *session = NSURLSession.sharedSession;
    [[session dataTaskWithURL:modelsURL completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
        if (error || !data || ![(NSHTTPURLResponse *)response statusCode] || [(NSHTTPURLResponse *)response statusCode] != 200) {
            [self reply:@{ @"error": @"The model is not ready yet. Please try again." }];
            return;
        }
        NSDictionary *listing = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
        id models = [listing isKindOfClass:NSDictionary.class] ? listing[@"data"] : nil;
        id firstModel = [models isKindOfClass:NSArray.class] && [models count] ? models[0] : nil;
        id model = [firstModel isKindOfClass:NSDictionary.class] ? firstModel[@"id"] : nil;
        if (![model isKindOfClass:NSString.class]) {
            [self reply:@{ @"error": @"Could not identify the loaded model." }];
            return;
        }
        NSURL *url = [NSURL URLWithString:[NSString stringWithFormat:@"http://127.0.0.1:%u/v1/chat/completions", self.apiPort]];
        NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url];
        request.HTTPMethod = @"POST";
        request.timeoutInterval = 600;
        [request setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
        request.HTTPBody = [NSJSONSerialization dataWithJSONObject:@{ @"model": model, @"messages": messages, @"max_tokens": @256 }
                                                          options:0 error:nil];
        [[session dataTaskWithRequest:request completionHandler:^(NSData *body, NSURLResponse *response, NSError *error) {
            if (error || !body || [(NSHTTPURLResponse *)response statusCode] != 200) {
                [self reply:@{ @"error": @"The cat could not reply. Please try again." }];
                return;
            }
            NSDictionary *json = [NSJSONSerialization JSONObjectWithData:body options:0 error:nil];
            id choices = [json isKindOfClass:NSDictionary.class] ? json[@"choices"] : nil;
            id first = [choices isKindOfClass:NSArray.class] && [choices count] ? choices[0] : nil;
            id message = [first isKindOfClass:NSDictionary.class] ? first[@"message"] : nil;
            id text = [message isKindOfClass:NSDictionary.class] ? message[@"content"] : nil;
            [self reply:[text isKindOfClass:NSString.class] ? @{ @"text": text } : @{ @"error": @"The cat returned an unexpected response." }];
        }] resume];
    }] resume];
}

- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message {
    (void)controller;
    if (![message.body isKindOfClass:NSDictionary.class]) return;
    NSURL *source = message.frameInfo.request.URL;
    if (![source.host isEqualToString:@"127.0.0.1"] || source.port.unsignedShortValue != self.uiPort || !message.frameInfo.mainFrame) return;
    NSDictionary *body = message.body;
    NSString *type = body[@"type"];
    if (![type isKindOfClass:NSString.class]) return;
    if ([type isEqual:@"ready"]) {
        if (!self.uiReady && !self.uiFailed) {
            self.uiReady = YES;
            self.markReady(self.context);
        }
    } else if ([type isEqual:@"fatal"]) {
        [self fail:[body[@"message"] isKindOfClass:NSString.class] ? body[@"message"] : @"renderer error"];
    } else if ([type isEqual:@"menu"]) [self showMenu];
    else if ([type isEqual:@"showChat"] && [body[@"open"] isKindOfClass:NSNumber.class]) {
        NSRect frame = self.window.frame;
        frame.size.width = [body[@"open"] boolValue] ? 570 : 245;
        NSScreen *screen = self.window.screen ?: NSScreen.mainScreen;
        if (screen) frame.origin.x = MIN(frame.origin.x, NSMaxX(screen.visibleFrame) - frame.size.width);
        [self.window setFrame:frame display:YES];
    }
    else if ([type isEqual:@"dragStart"]) self.dragOrigin = self.window.frame.origin;
    else if ([type isEqual:@"drag"] && [body[@"dx"] isKindOfClass:NSNumber.class] && [body[@"dy"] isKindOfClass:NSNumber.class]) {
        NSPoint origin = NSMakePoint(self.dragOrigin.x + [body[@"dx"] doubleValue], self.dragOrigin.y - [body[@"dy"] doubleValue]);
        NSScreen *screen = self.window.screen ?: NSScreen.mainScreen;
        if (screen) {
            NSRect visible = screen.visibleFrame;
            origin.x = MAX(NSMinX(visible), MIN(origin.x, NSMaxX(visible) - self.window.frame.size.width));
            origin.y = MAX(NSMinY(visible), MIN(origin.y, NSMaxY(visible) - self.window.frame.size.height));
        }
        [self.window setFrameOrigin:origin];
    } else if ([type isEqual:@"chat"] && [body[@"messages"] isKindOfClass:NSArray.class]) [self chat:body[@"messages"]];
}
@end

int lai_pet_run(uint16_t uiPort, uint16_t apiPort, void *context, PetCheck shouldStop, PetCheck serverDone, PetStop requestStop, PetStop markReady) {
    @autoreleasepool {
        NSApplication *app = NSApplication.sharedApplication;
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        PetController *controller = [PetController new];
        controller.context = context;
        controller.shouldStop = shouldStop;
        controller.serverDone = serverDone;
        controller.requestStop = requestStop;
        controller.markReady = markReady;
        controller.apiPort = apiPort;
        [controller start:uiPort];
        [app run];
        [controller.webView.configuration.userContentController removeScriptMessageHandlerForName:@"pet"];
        return controller.uiFailed ? 1 : 0;
    }
}
