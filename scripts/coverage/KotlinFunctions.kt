import java.io.File
import org.jetbrains.kotlin.cli.jvm.compiler.EnvironmentConfigFiles
import org.jetbrains.kotlin.cli.jvm.compiler.KotlinCoreEnvironment
import org.jetbrains.kotlin.config.CompilerConfiguration
import org.jetbrains.kotlin.com.intellij.openapi.util.Disposer
import org.jetbrains.kotlin.com.intellij.psi.PsiElement
import org.jetbrains.kotlin.com.intellij.psi.PsiErrorElement
import org.jetbrains.kotlin.com.intellij.psi.util.PsiTreeUtil
import org.jetbrains.kotlin.psi.*

fun functionName(element: PsiElement): String? = when (element) {
    is KtNamedFunction -> if (element.hasBody()) element.name ?: "<anonymous>" else null
    is KtPropertyAccessor -> if (element.hasBody()) (if (element.isGetter) "get:" else "set:") + element.property.name else null
    is KtSecondaryConstructor -> "<init>"
    is KtLambdaExpression -> "<lambda>"
    else -> null
}

fun reportFunction(source: String, path: String, element: PsiElement) {
    val name = functionName(element) ?: return
    val first = source.substring(0, element.textRange.startOffset).count { it == '\n' } + 1
    val last = source.substring(0, element.textRange.endOffset).count { it == '\n' } + 1
    val statement = (element as? KtLambdaExpression)?.bodyExpression?.statements?.firstOrNull()
    val body = statement?.let { source.substring(0, it.textRange.startOffset).count { c -> c == '\n' } + 1 } ?: first
    println("$path\t$first\t$last\t$name\t$body")
}

fun inventory(factory: KtPsiFactory, path: String) {
    val source = File(path).readText()
    val file = factory.createFile(File(path).name, source)
    check(PsiTreeUtil.findChildOfType(file, PsiErrorElement::class.java) == null) { "$path: Kotlin parse error" }
    for (element in PsiTreeUtil.collectElements(file) { functionName(it) != null }) {
        reportFunction(source, path, element)
    }
}

fun main(paths: Array<String>) {
    val disposable = Disposer.newDisposable()
    try {
        val environment = KotlinCoreEnvironment.createForProduction(disposable,
            CompilerConfiguration(), EnvironmentConfigFiles.JVM_CONFIG_FILES)
        val factory = KtPsiFactory(environment.project, false)
        paths.forEach { inventory(factory, it) }
    } finally { Disposer.dispose(disposable) }
}
