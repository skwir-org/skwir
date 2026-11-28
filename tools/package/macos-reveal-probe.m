// macos-reveal-probe.m: the native sender, and the canary, of macos-reveal-check.py.
//
// As a command it calls AppKit's and LaunchServices' own APIs exactly as a
// program does, and nothing else: no UI scripting, no Apple Events of its own.
//
//   probe activate <path>...    -[NSWorkspace activateFileViewerSelectingURLs:]
//   probe select <path> <root>  -[NSWorkspace selectFile:inFileViewerRootedAtPath:]
//                               (<root> empty: the file's own folder)
//   probe default <path>        -[NSWorkspace URLForApplicationToOpenURL:], read only:
//                               the bundle id that opens the path by default
//   probe lookup <bundle-id>    LSCopyApplicationURLsForBundleIdentifier and
//                               -[NSWorkspace URLForApplicationWithBundleIdentifier:]:
//                               where the system finds an application by its
//                               identifier, as a reveal's sender finds its NSFileViewer
//
// Started without a mode (as LaunchServices starts an application), it is the
// canary: a bundle around this program, with the candidate's declarations and
// LSEnvironment, that records what it was handed and the HOME, TMPDIR and
// XDG_RUNTIME_DIR it runs with, one JSON line each in canary.jsonl beside its
// bundle. The checker proves each sender's route and environment on it before
// Skwir is ever started. It records no other variable.
//
// Build (done by the checker, into its fixture folder):
//   clang -fobjc-arc -framework AppKit -framework CoreServices -o probe macos-reveal-probe.m
//
// A command prints one JSON line on standard output. NSWorkspace's reveal
// calls answer nothing about what opened, so the checker asserts the window's
// state, never this output.

#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>
#import <Foundation/Foundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// selectFile:inFileViewerRootedAtPath: is deprecated in the SDK but is the
// API under test here.
#pragma clang diagnostic ignored "-Wdeprecated-declarations"

static int usage(void) {
    fprintf(stderr, "usage: probe activate <path>... | select <path> <root> | default <path>"
                    " | lookup <bundle-id>\n");
    return 2;
}

static void print_json(id object) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:object options:0 error:nil];
    fwrite(data.bytes, 1, data.length, stdout);
    fputc('\n', stdout);
}

static id or_null(id value) { return value ?: [NSNull null]; }

static NSString *path_of(const char *arg) {
    return [[NSFileManager defaultManager] stringWithFileSystemRepresentation:arg
                                                                       length:strlen(arg)];
}

// The canary.

static void record(NSString *event, NSArray<NSURL *> *urls) {
    NSMutableArray *paths = [NSMutableArray array];
    for (NSURL *url in urls) [paths addObject:url.isFileURL ? url.path : url.absoluteString];
    NSMutableDictionary *env = [NSMutableDictionary dictionary];
    for (NSString *name in @[ @"HOME", @"TMPDIR", @"XDG_RUNTIME_DIR" ]) {
        const char *value = getenv(name.UTF8String);
        env[name] = value ? @(value) : [NSNull null];
    }
    NSData *line = [NSJSONSerialization
        dataWithJSONObject:@{@"event" : event, @"paths" : paths, @"env" : env}
                   options:0
                     error:nil];
    NSString *log = [[NSBundle.mainBundle.bundlePath stringByDeletingLastPathComponent]
        stringByAppendingPathComponent:@"canary.jsonl"];
    FILE *file = fopen(log.fileSystemRepresentation, "a");
    if (!file) return;
    fwrite(line.bytes, 1, line.length, file);
    fputc('\n', file);
    fclose(file);
}

@interface Canary : NSObject <NSApplicationDelegate>
@end

@implementation Canary
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    record(@"launched", @[]);
}
- (void)application:(NSApplication *)application openURLs:(NSArray<NSURL *> *)urls {
    record(@"opened", urls);
}
@end

static int canary(void) {
    NSApplication *app = [NSApplication sharedApplication];
    Canary *delegate = [Canary new];
    app.delegate = delegate;
    [app run];
    return 0;
}

int main(int argc, char *argv[]) {
    @autoreleasepool {
        if (argc < 2 || strncmp(argv[1], "-psn_", 5) == 0) return canary();
        if (argc < 3) return usage();
        NSString *mode = @(argv[1]);
        NSWorkspace *workspace = [NSWorkspace sharedWorkspace];

        if ([mode isEqualToString:@"activate"]) {
            NSMutableArray<NSURL *> *urls = [NSMutableArray array];
            for (int i = 2; i < argc; i++) [urls addObject:[NSURL fileURLWithPath:path_of(argv[i])]];
            [workspace activateFileViewerSelectingURLs:urls];
            print_json(@{@"sent" : @"activateFileViewerSelectingURLs:", @"count" : @(urls.count)});
            return 0;
        }

        if ([mode isEqualToString:@"select"] && argc == 4) {
            BOOL ok = [workspace selectFile:path_of(argv[2]) inFileViewerRootedAtPath:path_of(argv[3])];
            print_json(@{@"sent" : @"selectFile:inFileViewerRootedAtPath:", @"ok" : @(ok)});
            return 0;
        }

        if ([mode isEqualToString:@"default"] && argc == 3) {
            NSURL *app = [workspace URLForApplicationToOpenURL:[NSURL fileURLWithPath:path_of(argv[2])]];
            NSString *bundleId = app ? [NSBundle bundleWithURL:app].bundleIdentifier : nil;
            print_json(@{@"default_bundle_id" : or_null(bundleId)});
            return 0;
        }

        if ([mode isEqualToString:@"lookup"] && argc == 3) {
            NSString *bundleId = @(argv[2]);
            CFErrorRef error = NULL;
            NSArray<NSURL *> *found = CFBridgingRelease(
                LSCopyApplicationURLsForBundleIdentifier((__bridge CFStringRef)bundleId, &error));
            NSMutableArray *paths = [NSMutableArray array];
            for (NSURL *url in found) [paths addObject:url.path];
            NSURL *chosen = [workspace URLForApplicationWithBundleIdentifier:bundleId];
            print_json(@{
                @"launch_services" : paths,
                @"launch_services_error" : error ? @(CFErrorGetCode(error)) : [NSNull null],
                @"workspace" : or_null(chosen.path),
            });
            if (error) CFRelease(error);
            return 0;
        }
        return usage();
    }
}
