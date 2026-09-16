#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>

int main(int argc, const char *argv[]) {
  @autoreleasepool {
    if (argc != 2) {
      fprintf(stderr, "usage: window-proof <pid>\n");
      return 2;
    }
    pid_t pid = (pid_t)strtol(argv[1], NULL, 10);
    CFArrayRef copied = CGWindowListCopyWindowInfo(
      kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
      kCGNullWindowID
    );
    NSArray<NSDictionary *> *rows = CFBridgingRelease(copied);
    NSMutableArray<NSDictionary *> *candidates = [NSMutableArray array];
    for (NSDictionary *row in rows) {
      NSNumber *owner = row[(id)kCGWindowOwnerPID];
      NSNumber *layer = row[(id)kCGWindowLayer];
      NSString *title = row[(id)kCGWindowName];
      NSDictionary *bounds = row[(id)kCGWindowBounds];
      NSNumber *width = bounds[@"Width"];
      NSNumber *height = bounds[@"Height"];
      NSNumber *windowNumber = row[(id)kCGWindowNumber];
      if (owner.intValue == pid) {
        [candidates addObject:@{
          @"window_number": windowNumber ?: @0,
          @"title": title ?: [NSNull null],
          @"layer": layer ?: @0,
          @"width": width ?: @0,
          @"height": height ?: @0,
        }];
      }
      if (owner.intValue != pid || layer.intValue != 0 ||
          (title != nil && ![title isEqualToString:@"Mesh — Local workspace"]) ||
          width.doubleValue < 920 || height.doubleValue < 640) {
        continue;
      }
      NSDictionary *proof = @{
        @"schema": @"mesh-rendered-window-proof/v1",
        @"pid": owner,
        @"window_number": windowNumber,
        @"title": title ?: [NSNull null],
        @"width": width,
        @"height": height,
        @"on_screen": @YES,
      };
      NSData *encoded = [NSJSONSerialization dataWithJSONObject:proof options:NSJSONWritingSortedKeys error:nil];
      fwrite(encoded.bytes, 1, encoded.length, stdout);
      fputc('\n', stdout);
      return 0;
    }
    NSData *diagnostic = [NSJSONSerialization dataWithJSONObject:candidates options:NSJSONWritingSortedKeys error:nil];
    fprintf(stderr, "no matching on-screen Mesh window for pid %d; candidates=", pid);
    fwrite(diagnostic.bytes, 1, diagnostic.length, stderr);
    fputc('\n', stderr);
    return 1;
  }
}
