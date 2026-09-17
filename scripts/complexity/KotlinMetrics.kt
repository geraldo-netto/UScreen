import java.io.File
import org.jetbrains.kotlin.cli.jvm.compiler.EnvironmentConfigFiles
import org.jetbrains.kotlin.cli.jvm.compiler.KotlinCoreEnvironment
import org.jetbrains.kotlin.config.CompilerConfiguration
import org.jetbrains.kotlin.com.intellij.openapi.util.Disposer
import org.jetbrains.kotlin.com.intellij.psi.PsiElement
import org.jetbrains.kotlin.com.intellij.psi.PsiErrorElement
import org.jetbrains.kotlin.com.intellij.psi.util.PsiTreeUtil
import org.jetbrains.kotlin.lexer.KtTokens
import org.jetbrains.kotlin.psi.*

// Independently expressed count; see README.md for the pinned language rules.
fun increment(element: PsiElement): Int = when (element) {
    is KtNamedFunction -> if (element.hasBody() && element.name != null) 1 else 0
    is KtIfExpression, is KtLoopExpression, is KtWhenEntry -> 1
    is KtBinaryExpression -> if (element.operationToken in setOf(KtTokens.ANDAND, KtTokens.OROR)) 1 else 0
    else -> 0
}

fun complexity(element: PsiElement): Int = increment(element) + element.children.sumOf(::complexity)

fun report(factory: KtPsiFactory, path: String) {
    val source = File(path).readText()
    val file = factory.createFile(File(path).name, source)
    check(PsiTreeUtil.findChildOfType(file, PsiErrorElement::class.java) == null) { "$path: Kotlin parse error" }
    for (function in PsiTreeUtil.collectElementsOfType(file, KtNamedFunction::class.java)) {
        val line = source.substring(0, function.textOffset).count { it == '\n' } + 1
        println("$path\t$line\t${function.name}\t${complexity(function)}")
    }
}

fun main(paths: Array<String>) {
    val disposable = Disposer.newDisposable()
    try {
        val environment = KotlinCoreEnvironment.createForProduction(disposable,
            CompilerConfiguration(), EnvironmentConfigFiles.JVM_CONFIG_FILES)
        val factory = KtPsiFactory(environment.project, false)
        paths.forEach { report(factory, it) }
    } finally { Disposer.dispose(disposable) }
}
