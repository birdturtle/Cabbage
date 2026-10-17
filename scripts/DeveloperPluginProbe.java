import com.google.common.reflect.ClassPath;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Paths;
import net.runelite.client.plugins.Plugin;
import net.runelite.client.plugins.PluginDescriptor;

/** Offline packaging check using the same class enumeration as RuneLite's sideloader. */
class DeveloperPluginProbe {
    public static void main(String[] args) throws Exception {
        int plugins = 0;
        String prefix = "jar:" + Paths.get(args[0]).toUri().toURL() + "!/";
        try (URLClassLoader loader = new URLClassLoader(new URL[]{Paths.get(args[0]).toUri().toURL()},
                DeveloperPluginProbe.class.getClassLoader())) {
            for (ClassPath.ClassInfo info : ClassPath.from(loader).getAllClasses()) {
                if (info.url().toString().startsWith(prefix)) {
                    Class<?> type = info.load();
                    if (type.getAnnotation(PluginDescriptor.class) != null && type.getSuperclass() == Plugin.class) {
                        System.out.println("Discovered plugin: " + type.getName());
                        plugins++;
                    }
                }
            }
        }
        if (plugins == 0) throw new IllegalStateException("No RuneLite plugin was discovered");
        System.out.println("Plugin classes and packaged dependencies loaded successfully.");
    }
}
